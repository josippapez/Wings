/// <reference path="./wings.d.ts" />
/**
 * Calls `tick` every `ms` while Wings is focused. When Wings loses focus the timer stops; when it regains focus,
 * `tick` runs once at once and the timer starts again. `tick` is not called for the state Wings starts in, so
 * a plugin keeps doing its own first check. Returns a function that stops everything.
 */
export function pollWhileFocused(tick: () => void, ms: number, w?: Pick<Wings, "focused" | "onFocus">): () => void;
