import meta from "./commands.json" with { type: "json" };

/** The global shortcut that brings Wings forward and goes to the agent that needs you. */
export type GlobalHotkey = { enabled: boolean; accelerator: string };

export const DEFAULT_GLOBAL_HOTKEY: GlobalHotkey = { enabled: true, accelerator: meta.hotkey.default };

/** The keys a shortcut can be made of: what `KeyboardEvent.code` calls them, which the Rust side parses too. */
const KEY = /^(Key[A-Z]|Digit\d|F([1-9]|1[0-2])|Space|Arrow(Up|Down|Left|Right)|Backquote|Minus|Equal|Comma|Period|Slash|Semicolon|Quote|Backslash|BracketLeft|BracketRight)$/;

type Pressed = { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean };

/**
 * The accelerator for a key press, like "Ctrl+Alt+J", or null if it can't be a global shortcut: no key yet (only
 * modifiers held), or no Ctrl, Alt or Cmd, which would make a plain letter or Shift+letter take over typing everywhere.
 */
export function acceleratorFromEvent(e: Pressed): string | null {
  if (!KEY.test(e.code) || !(e.ctrlKey || e.altKey || e.metaKey)) return null;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Cmd", e.code].filter(Boolean).join("+");
}

const MAC_MODS: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Cmd: "⌘" };
const ARROWS: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };
const SYMBOLS: Record<string, string> = { Backquote: "`", Minus: "-", Equal: "=", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", Backslash: "\\", BracketLeft: "[", BracketRight: "]" };

/** "Ctrl+Alt+J" as the platform writes it: "⌃⌥J" on macOS, "Ctrl+Alt+J" elsewhere. */
export function hotkeyLabel(accelerator: string, mac: boolean): string {
  const parts = accelerator.split("+");
  const code = parts.pop() ?? "";
  const key = ARROWS[code] ?? SYMBOLS[code] ?? code.replace(/^(Key|Digit)/, "");
  if (!mac) return [...parts.map((m) => (m === "Cmd" ? "Win" : m)), key].join("+");
  return parts.map((m) => MAC_MODS[m] ?? m).join("") + key;
}
