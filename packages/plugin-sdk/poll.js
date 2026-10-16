// A timer that runs only while the user is in Wings.

/**
 * Calls `tick` every `ms` while Wings is focused. When Wings loses focus the timer stops; when it regains focus,
 * `tick` runs at once if `ms` has passed since the last one, so flipping between tabs doesn't fetch on every flip.
 * Otherwise the timer resumes for the time that was left. `tick` is not called for the state Wings starts in, so
 * a plugin keeps doing its own first check, unless `immediate` is set: then `tick` also runs once now, when Wings
 * starts focused (an unfocused start waits for focus like any other). `wings` is for tests. Returns a function
 * that stops everything.
 */
export function pollWhileFocused(tick, ms, { immediate = false, wings: w = globalThis.wings } = {}) {
  let timer;
  let wait;
  let wasFocused = w.focused;
  // An unfocused start has not ticked, so its first focus ticks.
  let last = wasFocused ? Date.now() : -Infinity;
  const run = () => {
    last = Date.now();
    tick();
  };
  const start = () => (timer ??= setInterval(run, ms));
  const stop = () => {
    clearInterval(timer);
    clearTimeout(wait);
    timer = wait = undefined;
  };
  if (wasFocused) {
    start();
    if (immediate) tick();
  }
  const off = w.onFocus(({ focused }) => {
    if (focused === wasFocused) return;
    wasFocused = focused;
    if (!focused) return stop();
    const left = ms - (Date.now() - last);
    if (left <= 0) {
      run();
      start();
    } else {
      wait = setTimeout(() => {
        wait = undefined;
        run();
        start();
      }, left);
    }
  });
  return () => {
    off();
    stop();
  };
}
