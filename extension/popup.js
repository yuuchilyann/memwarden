// Popup: tab list with per-tab discard, bulk "discard idle tabs" with a
// confirmation step, and the idle threshold. Titles are shown here only.

import { THRESHOLDS_MIN, canDiscard, discardIdle, discardTabs, getThreshold, isCandidate, setThreshold, summarize } from "./tabs.js";

const msg = (name, ...subs) => chrome.i18n.getMessage(name, subs.map(String));
const $ = (id) => document.getElementById(id);
const STATUS_POLL_MS = 1_000;
const RENDER_MS = 15_000;

for (const el of document.querySelectorAll("[data-i18n]")) {
  el.textContent = msg(el.dataset.i18n);
}
document.documentElement.lang = chrome.i18n.getUILanguage();

let candidates = 0;

function duration(ms) {
  const min = Math.floor(ms / 60_000);
  return min < 60 ? msg("durationMinutes", min) : msg("durationHours", Math.floor(min / 60));
}

async function refreshStatus() {
  try {
    const { status, detail } = await chrome.runtime.sendMessage({ type: "status" });
    const el = $("status");
    el.className = `status ${status}`;
    el.textContent = msg(
      { connected: "statusConnected", no_memwarden: "statusNoMemwarden", no_host: "statusNoHost" }[status] ??
        "statusConnecting",
    );
    // The browser's own error text (English), to tell a missing registration
    // from a host that failed to start.
    el.title = detail ?? "";
    $("status-detail").textContent = status === "no_host" ? (detail ?? "") : "";
  } catch {
    // The service worker is restarting; try again on the next poll.
  }
}

async function render() {
  const now = Date.now();
  const threshold = await getThreshold();
  const tabs = await chrome.tabs.query({});
  const counts = summarize(tabs, now, threshold);
  candidates = counts.candidates;

  $("summary").textContent = msg("summary", counts.total, counts.discarded, counts.candidates);
  const bulk = $("discard-idle");
  bulk.textContent = msg("discardIdle", candidates);
  bulk.disabled = candidates === 0;

  // Longest idle first; already discarded tabs at the end.
  tabs.sort((a, b) => Number(!!a.discarded) - Number(!!b.discarded) || (a.lastAccessed ?? now) - (b.lastAccessed ?? now));
  const list = $("tabs");
  list.replaceChildren(
    ...tabs.map((tab) => {
      const li = document.createElement("li");
      if (tab.discarded) li.className = "discarded";

      const title = document.createElement("span");
      title.className = "title";
      title.textContent = tab.title || msg("untitled");
      title.title = title.textContent;

      const meta = document.createElement("span");
      meta.className = isCandidate(tab, now, threshold) ? "meta candidate" : "meta";
      const parts = [];
      if (tab.discarded) parts.push(msg("discardedTab"));
      else if (tab.active) parts.push(msg("activeTab"));
      else if (typeof tab.lastAccessed === "number") parts.push(msg("idleFor", duration(now - tab.lastAccessed)));
      if (tab.audible) parts.push(msg("audibleTab"));
      if (tab.pinned) parts.push(msg("pinnedTab"));
      meta.textContent = parts.join(" · ");

      li.append(title, meta);
      if (canDiscard(tab)) {
        const button = document.createElement("button");
        button.textContent = msg("discardButton");
        button.addEventListener("click", async () => {
          button.disabled = true;
          await discardTabs([tab.id]);
          render();
        });
        li.append(button);
      }
      return li;
    }),
  );
}

async function setupThreshold() {
  const select = $("threshold");
  const current = await getThreshold();
  for (const min of THRESHOLDS_MIN) {
    const option = new Option(msg("durationMinutes", min), String(min), false, min === current);
    select.add(option);
  }
  select.addEventListener("change", async () => {
    await setThreshold(Number(select.value));
    render();
  });
}

function showConfirm(visible) {
  $("confirm").hidden = !visible;
  $("bulk").hidden = visible;
}

$("discard-idle").addEventListener("click", () => {
  $("result").hidden = true;
  $("confirm-text").textContent = msg("confirmText", candidates);
  showConfirm(true);
});
$("confirm-no").addEventListener("click", () => showConfirm(false));
$("confirm-yes").addEventListener("click", async () => {
  showConfirm(false);
  const { discarded, skipped } = await discardIdle();
  const result = $("result");
  result.textContent = msg("resultText", discarded, skipped);
  result.hidden = false;
  render();
});

await setupThreshold();
await Promise.all([render(), refreshStatus()]);
setInterval(refreshStatus, STATUS_POLL_MS);
setInterval(render, RENDER_MS);
