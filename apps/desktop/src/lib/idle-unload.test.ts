/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { canUnloadHistory, hiddenLoaded, MIN_HISTORY_LINES, stillUnloaded, type TerminalState } from "./idle-unload.ts";

const idle: TerminalState = { started: true, visible: false, restoring: false, alternate: false, historyLines: 10_000, unloaded: false };

test("a hidden terminal with a long history unloads it", () => {
  assert.equal(canUnloadHistory(idle), true);
});

test("a terminal that is on screen, unstarted, mid-restore or already unloaded keeps what it has", () => {
  for (const change of [{ visible: true }, { started: false }, { restoring: true }, { unloaded: true }]) {
    assert.equal(canUnloadHistory({ ...idle, ...change }), false, JSON.stringify(change));
  }
});

test("a full-screen program's pane is left alone, and a short history isn't worth a reset", () => {
  assert.equal(canUnloadHistory({ ...idle, alternate: true }), false);
  assert.equal(canUnloadHistory({ ...idle, historyLines: MIN_HISTORY_LINES - 1 }), false);
  assert.equal(canUnloadHistory({ ...idle, historyLines: MIN_HISTORY_LINES }), true);
});

test("only pages that are hidden and still loaded get a countdown", () => {
  assert.deepEqual(hiddenLoaded(["a", "b", "c"], "b", new Set(["c"])), ["a"]);
  assert.deepEqual(hiddenLoaded(["a", "b"], null, new Set()), ["a", "b"]);
});

test("a page that is shown again or gone stops being unloaded", () => {
  assert.deepEqual(stillUnloaded(new Set(["a", "b", "c"]), ["a", "b"], "b"), ["a"]);
  assert.deepEqual(stillUnloaded(new Set(), ["a"], null), []);
});
