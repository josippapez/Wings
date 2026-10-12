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
    pollWhileFocused(() => n++, 1000, w);
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
    const stop = pollWhileFocused(() => n++, 1000, w);
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
