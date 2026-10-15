// Typed messages between a plugin's main script and its pages, over `wings.broadcast`. Every message is wrapped in
// `{ "wings:sdk": 1, kind, ... }`, so these never mix with a plugin's own broadcasts. messages.d.ts has the API.

const SDK = "wings:sdk";
const ASK_TIMEOUT_MS = 30_000;
const SHARE_THROTTLE_MS = 300;

const messageOf = (error) => (error instanceof Error ? error.message : String(error));

/** Adds `listener` under `name` in `map`, and returns the function that removes it. */
function subscribe(map, name, listener) {
  let set = map.get(name);
  if (!set) map.set(name, (set = new Set()));
  set.add(listener);
  return () => void set.delete(listener);
}

/** Calls each listener of a set with `value`. One that throws doesn't stop the others. */
function deliver(set, value) {
  for (const listener of [...(set ?? [])]) {
    try {
      listener(value);
    } catch (error) {
      console.error(error);
    }
  }
}

/**
 * A set of named questions, events and shared states between the frames of one plugin. Make it in a module that main
 * and the pages both import. Nothing touches `wings` until the first call, so importing it costs nothing.
 */
export function channel({ wings } = {}) {
  const w = () => wings ?? globalThis.wings;
  // Frames hear each other's answers, so ids carry this channel's own random part.
  const self = Math.random().toString(36).slice(2);
  let count = 0;
  const asked = new Map();
  const answers = new Map();
  const events = new Map();
  const sharers = new Map();
  const watchers = new Map();
  let listening = false;

  const send = async (message) => w().broadcast({ [SDK]: 1, ...message });

  function settle(id, error, response) {
    const entry = asked.get(id);
    // Another frame's question, or one that already timed out.
    if (!entry) return;
    asked.delete(id);
    clearTimeout(entry.timer);
    if (error === undefined) entry.resolve(response);
    else entry.reject(error instanceof Error ? error : new Error(error));
  }

  async function answerAsk({ id, name, request }) {
    const handler = answers.get(name);
    if (!handler) return;
    let reply;
    try {
      reply = { response: await handler(request) };
    } catch (error) {
      reply = { error: messageOf(error) };
    }
    try {
      await send({ kind: "answer", id, ...reply });
    } catch (error) {
      // Wings refuses a broadcast over 64 KB. The asker hears why instead of waiting out its timeout.
      await send({ kind: "answer", id, error: `The answer to ${name} couldn't be sent: ${messageOf(error)}` });
    }
  }

  function receive(m) {
    if (m?.[SDK] !== 1) return;
    if (m.kind === "ask") answerAsk(m).catch(console.error);
    else if (m.kind === "answer") settle(m.id, m.error, m.response);
    else if (m.kind === "tell") deliver(events.get(m.name), m.payload);
    else if (m.kind === "want") sharers.get(m.name)?.(m.from);
    else if (m.kind === "state" && (m.to === undefined || m.to === self)) deliver(watchers.get(m.name), m.state);
  }

  const listen = () => {
    if (listening) return;
    listening = true;
    w().onBroadcast(receive);
  };

  return {
    answer(name, handler) {
      listen();
      answers.set(name, handler);
      return () => void (answers.get(name) === handler && answers.delete(name));
    },

    ask(name, request, { timeoutMs = ASK_TIMEOUT_MS } = {}) {
      listen();
      return new Promise((resolve, reject) => {
        const id = `${self}:${++count}`;
        const timer = setTimeout(() => settle(id, new Error("Wings didn't answer. Try again.")), timeoutMs);
        asked.set(id, { resolve, reject, timer });
        send({ kind: "ask", id, name, request }).catch((error) => settle(id, error));
      });
    },

    tell: (name, payload) => send({ kind: "tell", name, payload }),

    on(name, listener) {
      listen();
      return subscribe(events, name, listener);
    },

    share(name, snapshot, { throttleMs = SHARE_THROTTLE_MS } = {}) {
      listen();
      // The JSON of what every page last got, so an unchanged state sends nothing.
      let last;
      let timer;
      // A page that just opened asks for the state and gets it alone. `last` stays: the others have it already.
      sharers.set(name, (to) => send({ kind: "state", name, state: snapshot(), to }).catch(console.error));
      const flush = () => {
        timer = undefined;
        const state = snapshot();
        const json = JSON.stringify(state);
        if (json === last) return;
        last = json;
        send({ kind: "state", name, state }).catch((error) => {
          // Not sent, so the next change sends it again even if the state is the same.
          last = undefined;
          console.error(error);
        });
      };
      return { changed: () => void (timer ??= setTimeout(flush, throttleMs)) };
    },

    watch(name, listener) {
      listen();
      const off = subscribe(watchers, name, listener);
      send({ kind: "want", name, from: self }).catch(console.error);
      return off;
    },
  };
}
