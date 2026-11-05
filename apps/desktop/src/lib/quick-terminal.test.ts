/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { DEFAULT_QUICK_TERMINAL_KEY, INITIAL_LOOSE, isQuickTerminalKey, looseReducer } from "./quick-terminal.ts";

const press = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "metaKey" | "shiftKey", boolean>> = {}) => ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods });

const open = (...keys: string[]) => keys.reduce((s, key) => looseReducer(s, { type: "open", key, floating: false }), INITIAL_LOOSE);

test("each sidebar click opens another terminal, filling the pane area", () => {
  const s = open("a", "b");
  assert.deepEqual(s.list.map((t) => t.key), ["a", "b"]);
  assert.equal(s.page, "b");
  assert.equal(looseReducer(s, { type: "show", key: "a" }).page, "a");
});

test("going to a tab puts the full one away but keeps its row and shell", () => {
  const s = looseReducer(open("a"), { type: "leave" });
  assert.equal(s.page, null);
  assert.deepEqual(s.list, [{ key: "a", floating: false, minimised: false }]);
});

test("detached, it floats over the tabs and stays listed; clicking its row fills the area again", () => {
  const floating = looseReducer(open("a"), { type: "float", key: "a" });
  assert.equal(floating.page, null);
  assert.equal(floating.list[0].floating, true);
  assert.equal(looseReducer(floating, { type: "leave" }), floating);
  const back = looseReducer(floating, { type: "show", key: "a" });
  assert.equal(back.page, "a");
  assert.equal(back.list[0].floating, false);
});

test("closing the full one leaves the tab on screen and Ctrl+` on the newest left", () => {
  const s = looseReducer(open("a", "b"), { type: "close", key: "b" });
  assert.equal(s.page, null);
  assert.equal(s.last, "a");
  assert.deepEqual(looseReducer(s, { type: "close", key: "a" }), INITIAL_LOOSE);
});

test("Ctrl+` opens a floating one when there is none, then hides and shows the last one", () => {
  const first = looseReducer(INITIAL_LOOSE, { type: "toggle", fresh: "q" });
  assert.deepEqual(first, { list: [{ key: "q", floating: true, minimised: false }], page: null, last: "q" });
  const hidden = looseReducer(first, { type: "toggle", fresh: "r" });
  assert.equal(hidden.list[0].floating, false);
  assert.equal(looseReducer(hidden, { type: "toggle", fresh: "r" }).list[0].floating, true);
  const full = open("a");
  assert.equal(looseReducer(full, { type: "toggle", fresh: "r" }).page, null);
});

test("a minimised panel opens fully when floated again, and the full one can't be minimised", () => {
  const min = looseReducer(looseReducer(open("a"), { type: "float", key: "a" }), { type: "minimise", key: "a" });
  assert.equal(min.list[0].minimised, true);
  assert.equal(looseReducer(min, { type: "float", key: "a" }).list[0].minimised, false);
  const full = open("a");
  assert.equal(looseReducer(full, { type: "minimise", key: "a" }), full);
});

test("Ctrl+` matches by key code, whatever character the layout types, and nothing else does", () => {
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true }), DEFAULT_QUICK_TERMINAL_KEY), true);
  assert.equal(isQuickTerminalKey(press("Backquote"), DEFAULT_QUICK_TERMINAL_KEY), false);
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true, shiftKey: true }), DEFAULT_QUICK_TERMINAL_KEY), false);
  assert.equal(isQuickTerminalKey(press("Backquote", { ctrlKey: true }), { ...DEFAULT_QUICK_TERMINAL_KEY, enabled: false }), false);
});
