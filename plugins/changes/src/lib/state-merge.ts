import type { StateMessage } from "./types";

/** What main sent last, or what the page made of it while it waits. */
export type ChangesView = Omit<StateMessage, "type">;

/**
 * The Changes tab's view once a `state` message arrives. For the same repo and mode, a refresh (loading) or an error
 * keeps the file list on screen, so the list isn't unmounted and its scroll position isn't lost; the error rides along
 * as a notice, and any later good state replaces the view and so clears it. A different repo or mode, or no repo,
 * replaces the view.
 */
export function mergeState(prev: ChangesView | null, m: StateMessage): ChangesView {
  const same = prev && prev.mode === m.mode && prev.repo && m.repo && prev.repo.root === m.repo.root;
  if (same && m.loading) return { ...prev, loading: true };
  if (same && m.error && prev.files) return { ...prev, loading: false, error: m.error };
  return m;
}
