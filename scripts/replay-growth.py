"""Replays the growth.rs rules on a log from growth-logger.ps1, for several threshold sets.

    python -I scripts/replay-growth.py reports/growth-log.csv [--no-rise]

Prints the largest floor increase per app, which apps each threshold set flags
and when, and each large app's floor at the start and end of the log.
--no-rise drops the "still rising" condition (the rule before 2026-11-01).

The constants mirror growth.rs and the excluded names mirror the known lists in
classify.rs (services in session 0 are not in the log, so check flagged names by
hand). A restart is approximated as the app's earliest process start moving
later, or a missing minute. Standard library only.
"""
import csv
import sys
from collections import defaultdict

MB = 1024 * 1024
KEEP, RECENT, RISE_MINUTES, RISE_HOURS, MIN_RISE = 360, 10, 60, 2, 20 * MB
SETS = [("500MB x1.5 (current)", 500 * MB, 1.5), ("500MB x1.3", 500 * MB, 1.3), ("300MB x1.5", 300 * MB, 1.5),
        ("300MB x1.3", 300 * MB, 1.3), ("200MB x1.3", 200 * MB, 1.3)]

# Never tracked by growth.rs: system, security, virtual machine and browser names
# from classify.rs, plus pwsh.exe, which runs the logger.
EXCLUDE = set("""system registry smss.exe csrss.exe wininit.exe winlogon.exe services.exe lsass.exe lsaiso.exe
svchost.exe dwm.exe fontdrvhost.exe sihost.exe ctfmon.exe taskhostw.exe runtimebroker.exe searchindexer.exe
searchhost.exe spoolsv.exe conhost.exe dllhost.exe audiodg.exe wudfhost.exe explorer.exe shellexperiencehost.exe
startmenuexperiencehost.exe textinputhost.exe lockapp.exe applicationframehost.exe systemsettings.exe wmiprvse.exe
dashost.exe smartscreen.exe msmpeng.exe nissrv.exe mpdefendercoreservice.exe securityhealthservice.exe
securityhealthsystray.exe avp.exe avpui.exe adguardsvc.exe adguard.exe mbamservice.exe mbam.exe ekrn.exe
bdservicehost.exe ccsvchst.exe glasswire.exe gwctlsrv.exe vmmem vmmemcmzygote vmmemwsl vmwp.exe vmcompute.exe
virtualboxvm.exe vboxsvc.exe vmware-vmx.exe qemu-system-x86_64.exe chrome.exe msedge.exe firefox.exe brave.exe
opera.exe vivaldi.exe whale.exe arc.exe thorium.exe librewolf.exe waterfox.exe pwsh.exe""".split())
EXCLUDE |= {"secure system", "memory compression", "system idle process"}

args = [a for a in sys.argv[1:] if not a.startswith("--")]
RISE = "--no-rise" not in sys.argv
OBSERVED = RISE_MINUTES * RISE_HOURS if RISE else 60

series = defaultdict(list)  # app -> [(minute index, bytes, earliest start)]
minutes, index = [], {}
with open(args[0], encoding="utf-8-sig") as f:
    for r in csv.DictReader(f):
        if r["minute"] not in index:
            index[r["minute"]] = len(minutes)
            minutes.append(r["minute"])
        series[r["app"]].append((index[r["minute"]], int(r["private_bytes"]), r["first_start"]))


def histories(points):
    """Splits an app's points where memwarden would start a new history."""
    out, cur = [], []
    for p in points:
        if cur and (p[0] != cur[-1][0] + 1 or (p[2] and cur[-1][2] and p[2] > cur[-1][2] and p[2] > minutes[cur[-1][0]])):
            out.append(cur)
            cur = []
        cur.append(p)
    return out + [cur] if cur else out


def replay(points, inc, ratio):
    """Flagged minute indexes, and (low, current, minute) where the floor rose the most."""
    flagged, best = [], None
    bs = [b for _, b, _ in points]  # contiguous minutes: position == minutes since start

    def floor_at(end):
        return min(bs[max(0, end - RECENT + 1): end + 1])

    for now in range(OBSERVED, len(bs)):
        cur = floor_at(now)
        low = min(bs[max(0, now - KEEP + 1): now - RECENT + 1])
        rising, later = True, cur
        for h in range(1, RISE_HOURS + 1) if RISE else ():
            earlier = floor_at(now - h * RISE_MINUTES)
            rising &= later >= earlier + MIN_RISE
            later = earlier
        if rising and cur - low >= inc and cur >= low * ratio:
            flagged.append(points[now][0])
        if best is None or cur - low > best[1] - best[0]:
            best = (low, cur, points[now][0])
    return flagged, best


tracked = {app: histories(pts) for app, pts in series.items() if app not in EXCLUDE}
print(f"log: {minutes[0]} .. {minutes[-1]}, {len(minutes)} minutes, {len(series)} apps; "
      f"still-rising condition {'on' if RISE else 'off'}\n")

rows = []
for app, hs in tracked.items():
    for h in hs:
        _, best = replay(h, 1 << 62, 99)
        if best:
            rows.append((best[1] - best[0], app, *best, minutes[h[0][0]], len(h)))
rows.sort(reverse=True)
print(f"Largest floor increase within the {KEEP // 60} h window (no thresholds):")
print(f"{'app':32} {'low MB':>7} {'cur MB':>7} {'+MB':>6} {'ratio':>5}  at                history from      mins")
for d, app, low, cur, at, start, n in rows[:25]:
    print(f"{app:32} {low / MB:7.0f} {cur / MB:7.0f} {d / MB:6.0f} {cur / max(low, 1):5.2f}  {minutes[at]}  {start}  {n}")

print()
for name, inc, ratio in SETS:
    hits = []
    for app, hs in tracked.items():
        for h in hs:
            fl, _ = replay(h, inc, ratio)
            if fl:
                hits.append(f"{app} ({len(fl)} min, {minutes[fl[0]][11:]}..{minutes[fl[-1]][11:]})")
    print(f"[{name}] flagged: " + ("; ".join(hits) or "none"))

print("\nWhole log, first vs last 10-minute floor, apps above 300 MB:")
for app, pts in sorted(series.items(), key=lambda kv: -max(b for _, b, _ in kv[1])):
    if max(b for _, b, _ in pts) < 300 * MB:
        continue
    a, z = min(b for _, b, _ in pts[:10]), min(b for _, b, _ in pts[-10:])
    print(f"{app:32} {a / MB:7.0f} -> {z / MB:7.0f} MB ({(z - a) / MB:+6.0f})  histories={len(histories(pts))}"
          + ("  [not tracked]" if app in EXCLUDE else ""))
