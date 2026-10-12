/**
 * When off-screen things give their memory back. A terminal's 10,000 lines of scrollback cost about 17 MB per pane at
 * 140 columns (measured), a plugin page is a whole document. Each unloads after it has been hidden this long, and
 * comes back the next time it's shown.
 */
export const TERMINAL_UNLOAD_MS = 10 * 60_000;
export const SIDEBAR_UNLOAD_MS = 5 * 60_000;
export const TAB_PAGE_UNLOAD_MS = 10 * 60_000;
export const POPOVER_UNLOAD_MS = 2 * 60_000;

/** Fewer lines than this and serializing them saves too little to be worth a reset on restore. */
export const MIN_HISTORY_LINES = 500;

export type TerminalState = {
  started: boolean;
  visible: boolean;
  /** A restore is rewriting the buffer right now. */
  restoring: boolean;
  /** A full-screen program is showing: its screen is the alternate buffer, so the history isn't what's on screen. */
  alternate: boolean;
  historyLines: number;
  /** The history is already stored away. */
  unloaded: boolean;
};

export function canUnloadHistory(s: TerminalState) {
  return s.started && !s.visible && !s.restoring && !s.alternate && !s.unloaded && s.historyLines >= MIN_HISTORY_LINES;
}

/** Written between the old history and the tail (what arrived since, and the screen): a style reset and a line break. */
export const HISTORY_SEAM = "\x1b[0m\r\n";

/** Keys of pages that are hidden and still loaded: the ones to start a countdown for. */
export const hiddenLoaded = (keys: string[], shown: string | null, unloaded: ReadonlySet<string>) => keys.filter((k) => k !== shown && !unloaded.has(k));

/** The unloaded keys that stay unloaded: a page that's shown again, or gone, is dropped so it loads fresh. */
export const stillUnloaded = (unloaded: ReadonlySet<string>, keys: string[], shown: string | null) => [...unloaded].filter((k) => k !== shown && keys.includes(k));
