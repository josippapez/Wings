import { acceleratorFromEvent } from "./hotkey.ts";

/** The pane of a terminal that belongs to no project. The Rust side starts it in the home folder (`pty::NO_SPACE`). */
export const NO_SPACE = "";

/** The shortcut that shows and hides the quick terminal, matched on the key's `code` like the global shortcut. */
export type QuickTerminalKey = { enabled: boolean; accelerator: string };
export const DEFAULT_QUICK_TERMINAL_KEY: QuickTerminalKey = { enabled: true, accelerator: "Ctrl+Backquote" };

export const isQuickTerminalKey = (e: Parameters<typeof acceleratorFromEvent>[0], key: QuickTerminalKey) => key.enabled && acceleratorFromEvent(e) === key.accelerator;

/** A terminal in the home folder that belongs to no project, listed in the sidebar while its shell runs. */
export type LooseTerminal = { key: string; floating: boolean; minimised: boolean };

/**
 * The sidebar's terminals. `page` is the one filling the pane area in place of the tab. A floating one sits over
 * whatever is on screen. Neither, it's out of sight with its shell still running. `last` is the one Ctrl+` acts on.
 */
export type LooseState = { list: LooseTerminal[]; page: string | null; last: string | null };
export const INITIAL_LOOSE: LooseState = { list: [], page: null, last: null };

export type LooseAction =
  | { type: "open"; key: string; floating: boolean }
  | { type: "show" | "float" | "hide" | "minimise" | "expand" | "close"; key: string }
  /** Going to a tab or project. */
  | { type: "leave" }
  /** Ctrl+`: shows or hides the last one as a floating panel, or opens `fresh` when there is none. */
  | { type: "toggle"; fresh: string };

const update = (state: LooseState, key: string, change: Partial<LooseTerminal>) => ({ ...state, list: state.list.map((t) => (t.key === key ? { ...t, ...change } : t)) });

/** The terminal Ctrl+` acts on, or none, when it would open a new one. */
export const toggleTarget = (state: LooseState) => state.list.find((t) => t.key === state.last) ?? null;

export function looseReducer(state: LooseState, action: LooseAction): LooseState {
  if (action.type === "leave") return state.page ? { ...state, page: null } : state;
  if (action.type === "open") {
    const list = [...state.list, { key: action.key, floating: action.floating, minimised: false }];
    return { list, page: action.floating ? state.page : action.key, last: action.key };
  }
  if (action.type === "toggle") {
    const target = toggleTarget(state);
    if (!target) return looseReducer(state, { type: "open", key: action.fresh, floating: true });
    const shown = state.page === target.key || target.floating;
    return looseReducer(state, { type: shown ? "hide" : "float", key: target.key });
  }
  const { key } = action;
  if (!state.list.some((t) => t.key === key)) return state;
  const away = state.page === key ? null : state.page;
  switch (action.type) {
    case "show":
      return { ...update(state, key, { floating: false, minimised: false }), page: key, last: key };
    case "float":
      return { ...update(state, key, { floating: true, minimised: false }), page: away, last: key };
    case "hide":
      return { ...update(state, key, { floating: false }), page: away };
    case "minimise":
      return state.page === key ? state : update(state, key, { minimised: true });
    case "expand":
      return { ...update(state, key, { minimised: false }), last: key };
    case "close": {
      const list = state.list.filter((t) => t.key !== key);
      return { list, page: away, last: state.last === key ? (list.at(-1)?.key ?? null) : state.last };
    }
  }
}
