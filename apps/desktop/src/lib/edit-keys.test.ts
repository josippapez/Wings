/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { editKey } from "./edit-keys.ts";

const key = (k: string, mods: Partial<Record<"metaKey" | "altKey" | "ctrlKey" | "shiftKey", boolean>> = {}) =>
  editKey({ key: k, metaKey: false, altKey: false, ctrlKey: false, shiftKey: false, ...mods });

test("⌥← and ⌥→ move a word the way the shell binds it", () => {
  assert.equal(key("ArrowLeft", { altKey: true }), "\x1bb");
  assert.equal(key("ArrowRight", { altKey: true }), "\x1bf");
});

test("⌘← and ⌘→ go to the line's start and end, ⌘⌫ deletes to its start", () => {
  assert.equal(key("ArrowLeft", { metaKey: true }), "\x01");
  assert.equal(key("ArrowRight", { metaKey: true }), "\x05");
  assert.equal(key("Backspace", { metaKey: true }), "\x15");
});

test("other keys and combinations are left to xterm.js and Wings' shortcuts", () => {
  assert.equal(key("ArrowLeft"), null);
  assert.equal(key("ArrowLeft", { metaKey: true, altKey: true }), null); // focus the pane to the left
  assert.equal(key("ArrowLeft", { altKey: true, shiftKey: true }), null);
  assert.equal(key("ArrowUp", { altKey: true }), null);
  assert.equal(key("Backspace", { altKey: true }), null);
});
