import assert from "node:assert/strict";
import { mock, test } from "node:test";

import { pollWhileFocused } from "./poll.js";

function fakeWings(focused) {
  const listeners = [];
  return {
    focused,
    onFocus(fn) {
      listeners.push(fn);
      return () => listeners.splice(listeners.indexOf(fn), 1);
    },
    set(next) {
      this.focused = next;
      listeners.forEach((fn) => fn({ focused: next }));
    },
    listeners,
  };
}

test("ticks on the interval while focused, stops on blur, checks once and resumes on focus", () => {
  mock.timers.enable({ apis: ["setInterval"] });
  try {
    const w = fakeWings(true);
    let n = 0;
    pollWhileFocused(() => n++, 1000, { wings: w });
    mock.timers.tick(3000);
    assert.equal(n, 3);
    w.set(false);
    mock.timers.tick(10_000);
    assert.equal(n, 3);
    w.set(true);
    assert.equal(n, 4);
    mock.timers.tick(2000);
    assert.equal(n, 6);
  } finally {
    mock.timers.reset();
  }
});

test("starting unfocused waits for focus, and the stop function removes the listener and the timer", () => {
  mock.timers.enable({ apis: ["setInterval"] });
  try {
    const w = fakeWings(false);
    let n = 0;
    const stop = pollWhileFocused(() => n++, 1000, { wings: w });
    mock.timers.tick(5000);
    assert.equal(n, 0);
    w.set(true);
    assert.equal(n, 1);
    stop();
    mock.timers.tick(5000);
    assert.equal(n, 1);
    assert.equal(w.listeners.length, 0);
  } finally {
    mock.timers.reset();
  }
});

test("immediate ticks once at start when focused, and an unfocused start still waits for focus", () => {
  mock.timers.enable({ apis: ["setInterval"] });
  try {
    let n = 0;
    pollWhileFocused(() => n++, 1000, { immediate: true, wings: fakeWings(true) });
    assert.equal(n, 1);
    mock.timers.tick(1000);
    assert.equal(n, 2);
    const w = fakeWings(false);
    pollWhileFocused(() => n++, 1000, { immediate: true, wings: w });
    assert.equal(n, 2);
    w.set(true);
    assert.equal(n, 3);
  } finally {
    mock.timers.reset();
  }
});
