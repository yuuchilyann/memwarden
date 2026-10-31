// Run with: node --test "extension/tests/*.test.mjs"
// Covers the tab rules and that every locale has the same messages.

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";

import { canDiscard, isCandidate, summarize } from "../tabs.js";

const dir = new URL("../", import.meta.url);
const MIN = 60_000;
const now = 100 * MIN;

// An idle background tab: a candidate at a 15-minute threshold.
const idle = { id: 1, active: false, audible: false, pinned: false, discarded: false, status: "complete", lastAccessed: now - 20 * MIN };

test("idle background tab is a candidate", () => {
  assert.ok(isCandidate(idle, now, 15));
  assert.ok(!isCandidate(idle, now, 30), "not idle long enough");
});

test("protected tabs are never candidates", () => {
  for (const change of [
    { active: true },
    { audible: true },
    { pinned: true },
    { discarded: true },
    { autoDiscardable: false },
    { status: "loading" },
    { lastAccessed: undefined },
  ]) {
    assert.ok(!isCandidate({ ...idle, ...change }, now, 15), JSON.stringify(change));
  }
});

test("manual discard skips only active and discarded tabs", () => {
  assert.ok(canDiscard({ ...idle, audible: true, pinned: true }));
  assert.ok(!canDiscard({ ...idle, active: true }));
  assert.ok(!canDiscard({ ...idle, discarded: true }));
});

test("summary has counts only", () => {
  const tabs = [idle, { ...idle, id: 2, discarded: true }, { ...idle, id: 3, active: true }];
  assert.deepEqual(summarize(tabs, now, 15), { total: 3, discarded: 1, candidates: 1 });
});

const read = (path) => readFileSync(new URL(path, dir), "utf8");
const locales = Object.fromEntries(
  readdirSync(new URL("_locales/", dir)).map((lang) => [lang, JSON.parse(read(`_locales/${lang}/messages.json`))]),
);

test("all four locales define the same messages and placeholders", () => {
  assert.deepEqual(Object.keys(locales).sort(), ["en", "ja", "ko", "zh_TW"]);
  const shape = (messages) =>
    Object.fromEntries(Object.entries(messages).map(([k, v]) => [k, Object.keys(v.placeholders ?? {})]));
  for (const [lang, messages] of Object.entries(locales)) {
    assert.deepEqual(shape(messages), shape(locales.en), lang);
  }
});

test("every message referenced by the extension exists", () => {
  const sources = ["manifest.json", "popup.html", "popup.js"].map(read).join("\n");
  const used = new Set();
  for (const re of [/__MSG_(\w+)__/g, /data-i18n="(\w+)"/g, /msg\(\s*"(\w+)"/g, /"(status\w+)"/g]) {
    for (const m of sources.matchAll(re)) used.add(m[1]);
  }
  const missing = [...used].filter((k) => !(k in locales.en));
  assert.deepEqual(missing, []);
  const unused = Object.keys(locales.en).filter((k) => !used.has(k));
  assert.deepEqual(unused, []);
});
