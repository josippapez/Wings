// A timer that runs only while the user is in Wings.

/**
 * Calls `tick` every `ms` while Wings is focused. When Wings loses focus the timer stops; when it regains focus,
 * `tick` runs once at once and the timer starts again. `tick` is not called for the state Wings starts in, so
 * a plugin keeps doing its own first check. Returns a function that stops everything.
 */
export function pollWhileFocused(tick, ms, w = globalThis.wings) {
  let timer;
  let wasFocused = w.focused;
  const start = () => (timer ??= setInterval(tick, ms));
  const stop = () => {
    clearInterval(timer);
    timer = undefined;
  };
  if (wasFocused) start();
  const off = w.onFocus(({ focused }) => {
    if (focused === wasFocused) return;
    wasFocused = focused;
    if (!focused) return stop();
    tick();
    start();
  });
  return () => {
    off();
    stop();
  };
}
