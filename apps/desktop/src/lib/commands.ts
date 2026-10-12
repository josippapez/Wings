import meta from "./commands.json" with { type: "json" };

/** What each command is, apart from what it does: `commands.json`, which the menu bar reads too. */
export type CommandMeta = {
  id: string;
  title: string;
  group: "Tab" | "Pane" | "View" | "App";
  menu?: "Tab" | "Pane" | "View";
  gap?: boolean;
  key?: string;
  also?: string[];
  extra?: boolean;
  alt?: boolean;
  native?: boolean;
  plain?: boolean;
  devOnly?: boolean;
};

export const COMMAND_META = meta.commands as CommandMeta[];

/** A command: what it is, whether it can run now, and what it does. */
export type Command = CommandMeta & { enabled: boolean; run: () => void };

/** The parts of a key event the shortcuts read. */
export type KeyInput = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">;

/** Physical keys, for layouts that don't type these characters (Croatian-PC puts [ ] \ + - elsewhere). */
const FROM_CODE: Record<string, string> = {
  KeyT: "t", KeyD: "d", KeyB: "b", KeyJ: "j", KeyF: "f", KeyR: "r", KeyK: "k",
  Backslash: "\\", NumpadAdd: "+", NumpadSubtract: "-", Numpad0: "0",
  BracketLeft: "[", BracketRight: "]",
};

export const TAB_SHORTCUT = /^[1-9]$/;

/**
 * The command a key press is, if any. App shortcuts are ⌘ on macOS and Ctrl+Shift elsewhere (plain Ctrl belongs to
 * the shell). They match the character typed, like macOS menus do, so they work on any keyboard layout; the physical
 * key is only the fallback for layouts that don't type Latin characters. A tab shortcut is `tab:<index>`.
 */
export function commandForKey(e: KeyInput, mac: boolean, dev: boolean, commands: CommandMeta[] = COMMAND_META): string | null {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && e.shiftKey && !e.metaKey;
  if (!mod) return null;
  const extra = mac ? e.shiftKey : e.altKey;
  const typed = e.key.length === 1 && /[\x21-\x7e]/.test(e.key) ? e.key.toLowerCase() : null;
  const key = typed ?? FROM_CODE[e.code] ?? (/^Digit\d$/.test(e.code) ? e.code.slice(5) : null);
  for (const c of commands) {
    if (!c.key || c.native || (c.devOnly && !dev)) continue;
    if (c.alt) {
      if (e.altKey && e.code === c.key) return c.id;
      continue;
    }
    if (c.extra !== undefined && c.extra !== extra) continue;
    if (key && (key === c.key || c.also?.includes(key))) return c.id;
  }
  if (key && TAB_SHORTCUT.test(key)) return `tab:${Number(key) - 1}`;
  return null;
}

const ARROWS: Record<string, string> = { ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓" };

/** The shortcut as the platform writes it: "⇧⌘D" on macOS, "Ctrl+Shift+Alt+D" elsewhere. Empty if it has none. */
export function shortcutText(c: CommandMeta, mac: boolean): string {
  if (!c.key) return "";
  const key = ARROWS[c.key] ?? c.key.toUpperCase();
  if (mac) return `${c.alt ? "⌥" : ""}${c.extra ? "⇧" : ""}⌘${key}`;
  if (c.plain) return `Ctrl+${key}`;
  return `Ctrl+Shift${c.alt || c.extra ? "+Alt" : ""}+${key}`;
}

/** The label for a command's shortcut, by id. */
export function shortcutOf(id: string, mac: boolean): string {
  const c = COMMAND_META.find((m) => m.id === id);
  return c ? shortcutText(c, mac) : "";
}

/** What the app does for each command. `enabled` says whether it can run right now. */
export type CommandContext = {
  /** A project is open. */
  project: boolean;
  /** A terminal tab is on screen. */
  terminalTab: boolean;
  /** More than one tab in the project on screen. */
  tabs: number;
  dev: boolean;
  run: Record<string, () => void>;
};

/** Binds every command in `commands.json` to what the app does, or leaves it disabled if there is no handler. */
export function createCommands(ctx: CommandContext, commands: CommandMeta[] = COMMAND_META): Command[] {
  const needs: Record<string, boolean> = {
    "new-tab": ctx.project,
    "close-tab": ctx.project && ctx.tabs > 0,
    "next-tab": ctx.tabs > 1,
    "previous-tab": ctx.tabs > 1,
    "split-right": ctx.terminalTab,
    "split-down": ctx.terminalTab,
    "move-tab-left": ctx.tabs > 1,
    "move-tab-right": ctx.tabs > 1,
    "worktree-agent": ctx.project,
    "open-in-editor": ctx.project,
    "close-pane": ctx.tabs > 0,
    "zoom-pane": ctx.terminalTab,
    "focus-left": ctx.terminalTab,
    "focus-right": ctx.terminalTab,
    "focus-up": ctx.terminalTab,
    "focus-down": ctx.terminalTab,
    find: ctx.terminalTab,
  };
  return commands.map((c) => ({ ...c, enabled: !!ctx.run[c.id] && (needs[c.id] ?? true) && (!c.devOnly || ctx.dev), run: ctx.run[c.id] ?? (() => {}) }));
}
