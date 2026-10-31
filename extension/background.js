// Service worker: keeps a Native Messaging port to memwarden-host, reports
// tab counts to memwarden and discards idle tabs when memwarden's user asks.
// Only counts leave the extension; URLs and titles never do.

import { discardIdle, getThreshold, summarize } from "./tabs.js";

const HOST = "com.memwarden.host";
const PROTOCOL_VERSION = 1;
// At most one report per this interval while tabs keep changing.
const SEND_THROTTLE_MS = 5_000;
// Tabs become candidates just by aging, without any tab event.
const REFRESH_MS = 60_000;
// After the host went away, retry on the next tab event at most this often.
const RECONNECT_MS = 60_000;
// Opening the popup retries sooner, but not on every status poll.
const POPUP_RECONNECT_MS = 5_000;

let port = null;
/** "connecting" | "no_host" | "no_memwarden" | "connected" */
let status = "connecting";
/** The browser's message when the host could not be started; shown in the popup. */
let detail = "";
let lastAttempt = 0;
let sendTimer = null;
let refreshTimer = null;

// A retry keeps the last known status, so the popup shows why the previous
// attempt failed instead of flickering back to "connecting".
function connect() {
  if (port) return;
  lastAttempt = Date.now();
  let heard = false;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch (e) {
    detail = String(e?.message ?? e);
    setStatus("no_host");
    return;
  }
  port.onMessage.addListener((msg) => {
    heard = true;
    detail = "";
    onHostMessage(msg);
  });
  port.onDisconnect.addListener(() => {
    // Reading lastError also marks it as handled.
    const error = chrome.runtime.lastError?.message ?? "";
    port = null;
    // The host always reports a status first, so silence means it is not
    // registered or could not start.
    if (!heard) detail = error;
    setStatus(heard ? "no_memwarden" : "no_host");
  });
}

function setStatus(next) {
  status = next;
  clearInterval(refreshTimer);
  refreshTimer = null;
  if (status === "connected") {
    refreshTimer = setInterval(sendTabs, REFRESH_MS);
    sendTabs();
  }
}

function post(msg) {
  try {
    port?.postMessage({ v: PROTOCOL_VERSION, ...msg });
  } catch {
    // Disconnected meanwhile; onDisconnect updates the status.
  }
}

async function onHostMessage(msg) {
  if (msg?.v !== PROTOCOL_VERSION) return;
  if (msg.type === "status") {
    setStatus(msg.connected ? "connected" : "no_memwarden");
  } else if (msg.type === "discard_idle") {
    // Sent only when the user confirms the request in memwarden.
    const result = await discardIdle();
    post({ type: "discard_result", ...result });
    sendTabs();
  }
}

async function sendTabs() {
  if (status !== "connected") return;
  const threshold = await getThreshold();
  const tabs = await chrome.tabs.query({});
  post({ type: "tabs", ...summarize(tabs, Date.now(), threshold), threshold_min: threshold });
}

function onTabsChanged() {
  if (!port && Date.now() - lastAttempt >= RECONNECT_MS) connect();
  if (sendTimer) return;
  sendTimer = setTimeout(() => {
    sendTimer = null;
    sendTabs();
  }, SEND_THROTTLE_MS);
}

chrome.tabs.onCreated.addListener(onTabsChanged);
chrome.tabs.onRemoved.addListener(onTabsChanged);
chrome.tabs.onActivated.addListener(onTabsChanged);
chrome.tabs.onUpdated.addListener((_id, change) => {
  if ("status" in change || "discarded" in change || "audible" in change || "pinned" in change || "autoDiscardable" in change) {
    onTabsChanged();
  }
});
chrome.storage.onChanged.addListener(() => sendTabs());

// The popup asks for the connection status; opening it also retries the host.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "status") {
    if (!port && Date.now() - lastAttempt >= POPUP_RECONNECT_MS) connect();
    sendResponse({ status, detail });
  }
});

connect();
