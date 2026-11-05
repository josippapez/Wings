import { acceleratorFromEvent } from "./hotkey.ts";

/** The pane of a terminal that belongs to no project. The Rust side starts it in the home folder (`pty::NO_SPACE`). */
export const NO_SPACE = "";

/** The key the quick terminal's shell sits under in `terminals`. There is one. */
export const QUICK_TERMINAL_KEY = "quick-terminal";

/** The shortcut that shows and hides the quick terminal, matched on the key's `code` like the global shortcut. */
export type QuickTerminalKey = { enabled: boolean; accelerator: string };
export const DEFAULT_QUICK_TERMINAL_KEY: QuickTerminalKey = { enabled: true, accelerator: "Ctrl+Backquote" };

export const isQuickTerminalKey = (e: Parameters<typeof acceleratorFromEvent>[0], key: QuickTerminalKey) => key.enabled && acceleratorFromEvent(e) === key.accelerator;

/**
 * The quick terminal's panel. `off` has no shell; `hidden` keeps its shell running out of sight. `full` fills the pane
 * area like a tab instead of floating, and showing again keeps the mode it was last in. Showing always opens the panel
 * fully, since a minimised bar has no terminal to type in.
 */
export type QuickState = { phase: "off" | "shown" | "hidden"; minimised: boolean; full: boolean };
export const INITIAL_QUICK: QuickState = { phase: "off", minimised: false, full: false };
/** `page` is the sidebar row: it shows the terminal full, or hides it when it already is. `leave` is going to a tab or project. */
export type QuickAction = "toggle" | "page" | "fill" | "float" | "leave" | "minimise" | "expand" | "end";

export function quickReducer(state: QuickState, action: QuickAction): QuickState {
  const shown = state.phase === "shown";
  switch (action) {
    case "toggle":
      return shown ? { ...state, phase: "hidden" } : { phase: "shown", minimised: false, full: state.full };
    case "page":
      return shown && state.full ? { ...state, phase: "hidden" } : { phase: "shown", minimised: false, full: true };
    case "fill":
      return shown ? { ...state, minimised: false, full: true } : state;
    case "float":
      return shown ? { ...state, full: false } : state;
    case "leave":
      return shown && state.full ? { ...state, phase: "hidden" } : state;
    case "minimise":
      return shown && !state.full ? { ...state, minimised: true } : state;
    case "expand":
      return shown ? { ...state, minimised: false } : state;
    // The shell was closed, or exited on its own.
    case "end":
      return INITIAL_QUICK;
  }
}
