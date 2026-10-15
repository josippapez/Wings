// Frames of one plugin on an in-memory relay, for the tests: a broadcast reaches every other frame as JSON a tick
// later, over 64 KB it is refused, and the frames share one storage, as a plugin's do.

/** Returns a function that makes the next frame: `{ wings, sent }`, where `sent` is what it broadcast. */
export function relay() {
  const frames = [];
  const storage = new Map();
  return () => {
    const heard = new Set();
    const frame = {
      heard,
      sent: [],
      wings: {
        async broadcast(message) {
          const text = JSON.stringify(message ?? null);
          if (text.length > 64_000) throw new Error("A broadcast is limited to 64 KB");
          frame.sent.push(JSON.parse(text));
          for (const other of frames) if (other !== frame) queueMicrotask(() => other.heard.forEach((listener) => listener(JSON.parse(text))));
        },
        onBroadcast(listener) {
          heard.add(listener);
          return () => heard.delete(listener);
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
}

/** Waits until the messages in flight have been delivered and handled. */
export const settle = () => new Promise((resolve) => setImmediate(resolve));
