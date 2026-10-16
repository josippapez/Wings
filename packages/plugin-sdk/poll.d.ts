/// <reference path="./wings.d.ts" />
export interface PollOptions {
  /** Also run `tick` once now, when Wings starts focused. An unfocused start waits for focus like any other. */
  immediate?: boolean;
  /** For tests: a stand-in for the `wings` global. */
  wings?: Pick<Wings, "focused" | "onFocus">;
}

/**
 * Calls `tick` every `ms` while Wings is focused. When Wings loses focus the timer stops; when it regains focus,
 * `tick` runs at once if `ms` has passed since the last one, and the timer starts again. Within `ms` it waits for the
 * time that was left instead, so switching tabs back and forth doesn't fetch on every switch. `tick` is not called for the state Wings starts in, so
 * a plugin keeps doing its own first check, unless `immediate` is set: then `tick` also runs once now, when Wings
 * starts focused (an unfocused start waits for focus like any other). `wings` is for tests. Returns a function
 * that stops everything.
 */
export function pollWhileFocused(tick: () => void, ms: number, options?: PollOptions): () => void;
