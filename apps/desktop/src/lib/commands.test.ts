/// <reference types="node" />
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { COMMAND_META, commandForKey, createCommands, shortcutText, type KeyInput } from "./commands.ts";

const key = (init: Partial<KeyInput>): KeyInput => ({ key: "", code: "", metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...init });
const mac = (init: Partial<KeyInput>) => commandForKey(key({ metaKey: true, ...init }), true, true);
const other = (init: Partial<KeyInput>) => commandForKey(key({ ctrlKey: true, shiftKey: true, ...init }), false, true);

test("the shortcuts keep their keys on macOS", () => {
  assert.equal(mac({ key: "t", code: "KeyT" }), "new-tab");
  assert.equal(mac({ key: "d", code: "KeyD" }), "split-right");
  assert.equal(mac({ key: "D", code: "KeyD", shiftKey: true }), "split-down");
  assert.equal(mac({ key: "b", code: "KeyB" }), "toggle-sidebar");
  assert.equal(mac({ key: "\\", code: "Backslash" }), "toggle-sidebar");
  assert.equal(mac({ key: "j", code: "KeyJ" }), "next-agent");
  assert.equal(mac({ key: "J", code: "KeyJ", shiftKey: true }), "previous-agent");
  assert.equal(mac({ key: "f", code: "KeyF" }), "find");
  assert.equal(mac({ key: "k", code: "KeyK" }), "palette");
  assert.equal(mac({ key: "0", code: "Digit0" }), "font-reset");
  assert.equal(mac({ key: "r", code: "KeyR" }), "reload");
  assert.equal(mac({ key: "3", code: "Digit3" }), "tab:2");
  assert.equal(mac({ key: "ArrowLeft", code: "ArrowLeft", altKey: true }), "focus-left");
  assert.equal(mac({ key: "ArrowDown", code: "ArrowDown", altKey: true }), "focus-down");
});

test("the shortcuts keep their keys elsewhere, where Ctrl+Shift is the modifier and Alt the extra", () => {
  assert.equal(other({ key: "T", code: "KeyT" }), "new-tab");
  assert.equal(other({ key: "D", code: "KeyD" }), "split-right");
  assert.equal(other({ key: "D", code: "KeyD", altKey: true }), "split-down");
  assert.equal(other({ key: "J", code: "KeyJ", altKey: true }), "previous-agent");
  assert.equal(other({ key: "K", code: "KeyK" }), "palette");
  assert.equal(other({ key: "ArrowRight", code: "ArrowRight", altKey: true }), "focus-right");
});

test("plain Ctrl and plain keys belong to the shell", () => {
  assert.equal(commandForKey(key({ ctrlKey: true, key: "t", code: "KeyT" }), false, true), null);
  assert.equal(commandForKey(key({ key: "t", code: "KeyT" }), true, true), null);
  assert.equal(commandForKey(key({ metaKey: true, ctrlKey: true, key: "t", code: "KeyT" }), true, true), null);
});

test("+ and = both grow the font, - and _ shrink it, whether or not Shift is held", () => {
  for (const k of ["+", "="]) assert.equal(mac({ key: k, code: "Equal", shiftKey: k === "+" }), "font-up");
  for (const k of ["-", "_"]) assert.equal(mac({ key: k, code: "Minus", shiftKey: k === "_" }), "font-down");
});

test("a layout that types no Latin letter falls back to the physical key", () => {
  assert.equal(mac({ key: "т", code: "KeyT" }), "new-tab");
  assert.equal(mac({ key: "š", code: "BracketLeft" }), "previous-tab");
  assert.equal(mac({ key: "đ", code: "BracketRight" }), "next-tab");
  assert.equal(mac({ key: "+", code: "NumpadAdd" }), "font-up");
});

test("tab switching takes ] and [ with or without Shift", () => {
  assert.equal(mac({ key: "]", code: "BracketRight" }), "next-tab");
  assert.equal(mac({ key: "}", code: "BracketRight", shiftKey: true }), "next-tab");
  assert.equal(mac({ key: "{", code: "BracketLeft", shiftKey: true }), "previous-tab");
});

test("back and forward take the bracket keys with Alt, which tab switching leaves to them", () => {
  assert.equal(mac({ key: "“", code: "BracketLeft", altKey: true }), "go-back");
  assert.equal(mac({ key: "‘", code: "BracketRight", altKey: true }), "go-forward");
  assert.equal(other({ key: "[", code: "BracketLeft", altKey: true }), "go-back");
  assert.equal(shortcutText(COMMAND_META.find((c) => c.id === "go-back")!, true), "⌥⌘[");
  assert.equal(shortcutText(COMMAND_META.find((c) => c.id === "go-forward")!, false), "Ctrl+Shift+Alt+]");
});

test("reload is for development builds only, and not with the extra key", () => {
  assert.equal(commandForKey(key({ metaKey: true, key: "r", code: "KeyR" }), true, false), null);
  assert.equal(mac({ key: "R", code: "KeyR", shiftKey: true }), null);
});

test("the keys the menu owns are not matched by the window, so they don't run twice", () => {
  assert.equal(mac({ key: "w", code: "KeyW" }), null);
  assert.equal(mac({ key: ",", code: "Comma" }), null);
});

test("no two commands take the same key", () => {
  const seen = new Map<string, string>();
  for (const c of COMMAND_META.filter((c) => c.key && !c.native)) {
    for (const extra of [false, true]) {
      if (c.extra !== undefined && c.extra !== extra) continue;
      for (const k of [c.key!, ...(c.also ?? [])]) {
        const id = `${c.alt ? "alt " : ""}${k} extra=${extra}`;
        assert.equal(seen.get(id), undefined, `${c.id} and ${seen.get(id)} both take ${id}`);
        seen.set(id, c.id);
      }
    }
  }
});

test("shortcuts read the way the platform writes them", () => {
  const meta = (id: string) => COMMAND_META.find((c) => c.id === id)!;
  assert.equal(shortcutText(meta("new-tab"), true), "⌘T");
  assert.equal(shortcutText(meta("split-down"), true), "⇧⌘D");
  assert.equal(shortcutText(meta("split-down"), false), "Ctrl+Shift+Alt+D");
  assert.equal(shortcutText(meta("focus-left"), true), "⌥⌘←");
  assert.equal(shortcutText(meta("close-pane"), false), "Ctrl+Shift+W");
  assert.equal(shortcutText(meta("settings"), false), "Ctrl+,");
  assert.equal(shortcutText(meta("history"), true), "");
});

test("a command is enabled when the app can do it now", () => {
  const run = Object.fromEntries(COMMAND_META.map((c) => [c.id, () => {}]));
  const by = (ctx: Parameters<typeof createCommands>[0]) => Object.fromEntries(createCommands(ctx).map((c) => [c.id, c.enabled]));
  const empty = by({ project: false, terminalTab: false, tabs: 0, canGoBack: false, canGoForward: false, dev: false, run });
  assert.equal(empty["new-tab"], false);
  assert.equal(empty["split-right"], false);
  assert.equal(empty["toggle-sidebar"], true);
  assert.equal(empty.reload, false);
  assert.equal(empty["close-tab"], false);
  // A page opened with no project on screen is a tab to close.
  assert.equal(by({ project: false, terminalTab: false, tabs: 1, canGoBack: false, canGoForward: false, dev: false, run })["close-tab"], true);
  const one = by({ project: true, terminalTab: true, tabs: 1, canGoBack: false, canGoForward: false, dev: true, run });
  assert.equal(one["new-tab"], true);
  assert.equal(one["split-right"], true);
  assert.equal(one["next-tab"], false);
  assert.equal(one.reload, true);
  assert.equal(by({ project: true, terminalTab: true, tabs: 2, canGoBack: false, canGoForward: false, dev: false, run })["next-tab"], true);
});

test("back and forward are enabled only with somewhere to go", () => {
  const run = Object.fromEntries(COMMAND_META.map((c) => [c.id, () => {}]));
  const by = (canGoBack: boolean, canGoForward: boolean) => Object.fromEntries(createCommands({ project: true, terminalTab: true, tabs: 1, canGoBack, canGoForward, dev: false, run }).map((c) => [c.id, c.enabled]));
  assert.equal(by(false, false)["go-back"], false);
  assert.equal(by(false, false)["go-forward"], false);
  assert.equal(by(true, false)["go-back"], true);
  assert.equal(by(true, false)["go-forward"], false);
  assert.equal(by(false, true)["go-forward"], true);
});

test("a command with no handler is disabled", () => {
  const all = createCommands({ project: true, terminalTab: true, tabs: 3, canGoBack: false, canGoForward: false, dev: true, run: {} });
  assert.ok(all.every((c) => !c.enabled));
});

// The menu bar is built in Rust from the same file; these are the things that file has to keep true for it.
test("every menu item has a menu the Rust side knows, and every key it can turn into an accelerator", () => {
  const menus = new Set(COMMAND_META.flatMap((c) => (c.menu ? [c.menu] : [])));
  assert.deepEqual([...menus].sort(), ["Pane", "Tab", "View"]);
  const rust = readFileSync(new URL("../../src-tauri/src/menu.rs", import.meta.url), "utf8");
  for (const name of menus) assert.ok(rust.includes(`"${name}"`), `menu.rs doesn't build the ${name} menu`);
  assert.ok(rust.includes("src/lib/commands.json"), "menu.rs doesn't read commands.json");
});
