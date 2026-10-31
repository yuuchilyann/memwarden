// Tab rules shared by the service worker and the popup.
// URLs and titles are read only for display in the popup; nothing here
// sends them anywhere.

export const THRESHOLDS_MIN = [5, 15, 30, 60, 120];
export const DEFAULT_THRESHOLD_MIN = 15;
const THRESHOLD_KEY = "thresholdMin";

/** Whether `tab` would be discarded by "discard idle tabs" (PLAN.md 7.3). */
export function isCandidate(tab, now, thresholdMin) {
  return (
    !tab.active &&
    !tab.audible &&
    !tab.pinned &&
    !tab.discarded &&
    tab.autoDiscardable !== false &&
    tab.status === "complete" &&
    typeof tab.lastAccessed === "number" &&
    now - tab.lastAccessed >= thresholdMin * 60_000
  );
}

/** Whether the user may discard `tab` by hand: anything not visible or already unloaded. */
export function canDiscard(tab) {
  return !tab.active && !tab.discarded;
}

/** Counts sent to memwarden. Numbers only. */
export function summarize(tabs, now, thresholdMin) {
  return {
    total: tabs.length,
    discarded: tabs.filter((t) => t.discarded).length,
    candidates: tabs.filter((t) => isCandidate(t, now, thresholdMin)).length,
  };
}

export async function getThreshold() {
  try {
    const stored = (await chrome.storage.local.get(THRESHOLD_KEY))[THRESHOLD_KEY];
    return THRESHOLDS_MIN.includes(stored) ? stored : DEFAULT_THRESHOLD_MIN;
  } catch {
    return DEFAULT_THRESHOLD_MIN;
  }
}

export async function setThreshold(minutes) {
  if (THRESHOLDS_MIN.includes(minutes)) {
    await chrome.storage.local.set({ [THRESHOLD_KEY]: minutes });
  }
}

/** Discards the given tabs. The browser may refuse some; those count as skipped. */
export async function discardTabs(ids) {
  let discarded = 0;
  for (const id of ids) {
    try {
      if (await chrome.tabs.discard(id)) discarded++;
    } catch {
      // Tab closed meanwhile, or the browser refused.
    }
  }
  return { requested: ids.length, discarded, skipped: ids.length - discarded };
}

/** Discards every current candidate. */
export async function discardIdle() {
  const now = Date.now();
  const threshold = await getThreshold();
  const tabs = await chrome.tabs.query({});
  return discardTabs(tabs.filter((t) => isCandidate(t, now, threshold)).map((t) => t.id));
}
