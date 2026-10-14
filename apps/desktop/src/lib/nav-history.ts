/** Where you are in Wings: a project and one of its tabs. Sidebars and modals aren't places. */
export type Place = { space: string; tab: string };

/** The places you've been, oldest first, where you are, and the ones you went back from. */
export type NavState = { back: Place[]; current: Place | null; forward: Place[] };

export const EMPTY_NAV: NavState = { back: [], current: null, forward: [] };

/** How many places Back remembers. */
export const NAV_LIMIT = 50;

/** Whether a place can still be shown: its project and its tab are still open. */
export type Exists = (place: Place) => boolean;

const same = (a: Place | null, b: Place | null) => a?.space === b?.space && a?.tab === b?.tab;

/**
 * You are now at `place`. A different place than the current one goes onto the back stack and clears forward, like a
 * browser; the same place changes nothing, which is also what a Back or Forward looks like once it has been shown.
 */
export function visit(state: NavState, place: Place | null): NavState {
  if (!place || same(state.current, place)) return state;
  if (!state.current) return { back: [], current: place, forward: [] };
  const back = same(state.back[state.back.length - 1] ?? null, state.current) ? state.back : [...state.back, state.current];
  return { back: back.slice(-NAV_LIMIT), current: place, forward: [] };
}

/** The latest place in a stack to go to. Closed tabs and the place you're already at aren't somewhere to go. */
const lastReachable = (stack: Place[], state: NavState, exists: Exists) => {
  for (let i = stack.length - 1; i >= 0; i--) if (!same(stack[i], state.current) && exists(stack[i])) return i;
  return -1;
};


export const canGoBack = (state: NavState, exists: Exists) => lastReachable(state.back, state, exists) >= 0;
export const canGoForward = (state: NavState, exists: Exists) => lastReachable(state.forward, state, exists) >= 0;

/** Back to the latest place that still exists; the places skipped on the way are dropped. Nothing to go to returns the same state. */
export function goBack(state: NavState, exists: Exists): NavState {
  if (!state.current) return state;
  const at = lastReachable(state.back, state, exists);
  if (at < 0) return state;
  return { back: state.back.slice(0, at), current: state.back[at], forward: [...state.forward, state.current] };
}

/** The mirror of `goBack`, towards where you came from. */
export function goForward(state: NavState, exists: Exists): NavState {
  if (!state.current) return state;
  const at = lastReachable(state.forward, state, exists);
  if (at < 0) return state;
  return { back: [...state.back, state.current].slice(-NAV_LIMIT), current: state.forward[at], forward: state.forward.slice(0, at) };
}
