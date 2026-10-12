/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { acceleratorFromEvent, DEFAULT_GLOBAL_HOTKEY, hotkeyLabel } from "./hotkey.ts";

const press = (code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods });

test("a chord with Ctrl, Alt or Cmd becomes an accelerator, modifiers in a fixed order", () => {
  assert.equal(acceleratorFromEvent(press("KeyJ", { altKey: true, ctrlKey: true })), "Ctrl+Alt+KeyJ");
  assert.equal(acceleratorFromEvent(press("Space", { metaKey: true, shiftKey: true })), "Shift+Cmd+Space");
  assert.equal(acceleratorFromEvent(press("Digit1", { ctrlKey: true })), "Ctrl+Digit1");
  assert.equal(acceleratorFromEvent(press("F5", { altKey: true })), "Alt+F5");
});

test("a key that would take over typing is refused: no modifier, Shift alone, or only modifiers held", () => {
  assert.equal(acceleratorFromEvent(press("KeyJ")), null);
  assert.equal(acceleratorFromEvent(press("KeyJ", { shiftKey: true })), null);
  assert.equal(acceleratorFromEvent(press("ControlLeft", { ctrlKey: true })), null);
  assert.equal(acceleratorFromEvent(press("Escape", { ctrlKey: true })), null);
  assert.equal(acceleratorFromEvent(press("F13", { ctrlKey: true })), null);
});

test("labels read the way the platform writes them", () => {
  assert.equal(hotkeyLabel("Ctrl+Alt+KeyJ", true), "⌃⌥J");
  assert.equal(hotkeyLabel("Ctrl+Alt+KeyJ", false), "Ctrl+Alt+J");
  assert.equal(hotkeyLabel("Shift+Cmd+Space", true), "⇧⌘Space");
  assert.equal(hotkeyLabel("Ctrl+ArrowUp", true), "⌃↑");
  assert.equal(hotkeyLabel("Alt+BracketLeft", true), "⌥[");
});

test("the default is on, and a chord the recorder itself could make", () => {
  assert.equal(DEFAULT_GLOBAL_HOTKEY.enabled, true);
  assert.equal(DEFAULT_GLOBAL_HOTKEY.accelerator, "Ctrl+Alt+KeyJ");
  assert.equal(acceleratorFromEvent(press("KeyJ", { ctrlKey: true, altKey: true })), DEFAULT_GLOBAL_HOTKEY.accelerator);
});
