/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { DEFAULT_QUICK_TERMINAL_KEY, INITIAL_QUICK, isQuickTerminalKey, quickReducer, type QuickState } from "./quick-terminal.ts";

const press = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "metaKey" | "shiftKey", boolean>> = {}) => ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods });

test("the panel shows, hides and shows again, the shell kept in between", () => {
  const shown = quickReducer(INITIAL_QUICK, "toggle");
  assert.deepEqual(shown, { phase: "shown", minimised: false, full: false });
  const hidden = quickReducer(shown, "toggle");
  assert.equal(hidden.phase, "hidden");
  assert.equal(quickReducer(hidden, "toggle").phase, "shown");
});

test("showing it again after hiding while minimised opens it fully, since a bar has no terminal to type in", () => {
  const hidden: QuickState = { phase: "hidden", minimised: true, full: false };
  assert.deepEqual(quickReducer(hidden, "toggle"), { phase: "shown", minimised: false, full: false });
});

test("minimise and expand only act while the panel is shown", () => {
  assert.equal(quickReducer({ phase: "shown", minimised: false, full: false }, "minimise").minimised, true);
  assert.equal(quickReducer({ phase: "shown", minimised: true, full: false }, "expand").minimised, false);
  assert.deepEqual(quickReducer({ phase: "hidden", minimised: false, full: false }, "minimise"), { phase: "hidden", minimised: false, full: false });
});

test("once the shell ends the panel starts over, so the next toggle makes a fresh one", () => {
  assert.deepEqual(quickReducer({ phase: "shown", minimised: true, full: false }, "end"), INITIAL_QUICK);
});

test("the sidebar row opens it full, a second click hides it, and the shortcut brings it back full", () => {
  const floating: QuickState = { phase: "shown", minimised: true, full: false };
  const full = quickReducer(floating, "page");
  assert.deepEqual(full, { phase: "shown", minimised: false, full: true });
  const hidden = quickReducer(full, "page");
  assert.equal(hidden.phase, "hidden");
  assert.deepEqual(quickReducer(hidden, "toggle"), full);
  assert.deepEqual(quickReducer(INITIAL_QUICK, "page"), full);
});

test("going to a tab hides it only while it fills the area, and it floats or fills in place", () => {
  const full: QuickState = { phase: "shown", minimised: false, full: true };
  assert.equal(quickReducer(full, "leave").phase, "hidden");
  const floating = quickReducer(full, "float");
  assert.deepEqual(floating, { phase: "shown", minimised: false, full: false });
  assert.equal(quickReducer(floating, "leave"), floating);
  assert.deepEqual(quickReducer({ ...floating, minimised: true }, "fill"), full);
  assert.equal(quickReducer(full, "minimise"), full);
});

test("Ctrl+` matches by key code, whatever character the layout types, and nothing else does", () => {
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true }), DEFAULT_QUICK_TERMINAL_KEY), true);
  assert.equal(isQuickTerminalKey(press("Backquote"), DEFAULT_QUICK_TERMINAL_KEY), false);
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true, shiftKey: true }), DEFAULT_QUICK_TERMINAL_KEY), false);
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true }), { ...DEFAULT_QUICK_TERMINAL_KEY, enabled: false }), false);
});
