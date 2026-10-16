// Frames of one plugin on an in-memory relay, for the tests: a broadcast reaches every other open frame as JSON a tick
// later, over 64,000 characters it is refused, and the frames share one storage and one focus, as a plugin's do.

/** Returns a function that makes the next frame: `{ wings, sent, close() }`, where `sent` is what it broadcast. `.focus(on)` moves the focus. */
export function relay() {
  const frames = [];
  const storage = new Map();
  const focus = { focused: true, listeners: new Set() };
  const make = () => {
    const heard = new Set();
    const frame = {
      heard,
      sent: [],
      close: () => void frames.splice(frames.indexOf(frame), 1),
      wings: {
        async broadcast(message) {
          const text = JSON.stringify(message ?? null);
          if (text.length > 64_000) throw new Error("A broadcast is limited to 64 KB");
          frame.sent.push(JSON.parse(text));
          // A closed frame's timers may still run, but what it sends goes nowhere.
          if (!frames.includes(frame)) return;
          for (const other of frames) if (other !== frame) queueMicrotask(() => other.heard.forEach((listener) => listener(JSON.parse(text))));
        },
        onBroadcast(listener) {
          heard.add(listener);
          return () => heard.delete(listener);
        },
        get focused() {
          return focus.focused;
        },
        onFocus(listener) {
          focus.listeners.add(listener);
          return () => focus.listeners.delete(listener);
        },
        storage: {
          get: async (key) => storage.get(key) ?? null,
          set: async (key, value) => void storage.set(key, JSON.parse(JSON.stringify(value))),
        },
      },
    };
    frames.push(frame);
    return frame;
  };
  make.focus = (on) => {
    focus.focused = on;
    focus.listeners.forEach((listener) => listener({ focused: on }));
  };
  return make;
}

/** Waits until the messages in flight have been delivered and handled. */
export const settle = () => new Promise((resolve) => setImmediate(resolve));
