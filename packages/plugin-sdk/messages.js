// Typed messages between a plugin's main script and its pages, over `wings.broadcast`. Every message is wrapped in
// `{ "wings:sdk": 1, kind, ... }`, so these never mix with a plugin's own broadcasts. messages.d.ts has the API.
//
// Kinds: `ask`, `answer` and `tell`; for queries `up` (main is serving), `sub` and `unsub` (a page starts or stops
// showing keys), `state` and `fresh` (main loaded a key: a new value, or the same one again), `put` (a frame set a
// value), `refresh` (a page wants a reload); and `part`, a piece of any message too big for one broadcast.

import { pollWhileFocused } from "./poll.js";

const SDK = "wings:sdk";
const ASK_TIMEOUT_MS = 30_000;
/** Wings refuses a broadcast whose JSON is longer. */
const MAX_CHARS = 64_000;
/** A message whose parts haven't all come by then is dropped. */
const PART_TTL_MS = 30_000;
/** A page renews the keys it shows this often while Wings is focused, and main drops a lease this long after the last renewal. */
const RENEW_MS = 30_000;
const LEASE_MS = 90_000;
/** A key nobody shows is forgotten this long after its last use. */
const DROP_MS = 5 * 60_000;

const messageOf = (error) => (error instanceof Error ? error.message : String(error));

/** Adds `listener` under `name` in `map`, and returns the function that removes it. */
function subscribe(map, name, listener) {
  let set = map.get(name);
  if (!set) map.set(name, (set = new Set()));
  set.add(listener);
  return () => {
    set.delete(listener);
    if (!set.size && map.get(name) === set) map.delete(name);
  };
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

/** A key as text, the same whatever order its fields are in. */
function keyOf(key) {
  const sorted = (v) =>
    Array.isArray(v) ? v.map(sorted) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])])) : v;
  return JSON.stringify(sorted(key ?? null));
}
const idOf = (name, key) => `${name}\n${keyOf(key)}`;

/** `text` in slices whose JSON, escapes and all, is at most `room` characters. A slice never ends between the halves of a surrogate pair. */
function slices(text, room) {
  const out = [];
  let at = 0;
  while (at < text.length) {
    let size = Math.min(room, text.length - at);
    for (;;) {
      let end = at + size;
      const code = text.charCodeAt(end - 1);
      if (end < text.length && end - at > 1 && code >= 0xd800 && code <= 0xdbff) end--;
      const length = JSON.stringify(text.slice(at, end)).length - 2;
      if (length <= room) {
        out.push(text.slice(at, end));
        at = end;
        break;
      }
      size = Math.max(1, Math.floor((size * room) / length) - 1);
    }
  }
  return out;
}

/**
 * A set of named questions, events and queries between the frames of one plugin. Make it in a module that main and the
 * pages both import. Nothing touches `wings` until the first call, so importing it costs nothing.
 */
export function channel({ wings } = {}) {
  const w = () => wings ?? globalThis.wings;
  // Frames hear each other's answers, so ids carry this channel's own random part.
  const self = Math.random().toString(36).slice(2);
  let count = 0;
  const asked = new Map();
  const answers = new Map();
  const events = new Map();
  /** Messages coming in parts: id → the parts so far. */
  const partial = new Map();
  /** Main: the queries this frame serves, by name. */
  const served = new Map();
  /** A page: the keys this frame shows, by id. */
  const shown = new Map();
  /** A page: ids to sub or unsub on the next microtask. */
  const pending = new Map();
  let renewing;
  let listening = false;
  let up = false;
  let focusHeard = false;

  // ---------- transport ----------

  async function send(message) {
    const whole = { [SDK]: 1, ...message };
    const text = JSON.stringify(whole);
    if (text.length <= MAX_CHARS) return w().broadcast(whole);
    // Too big for one broadcast: in parts, each with what a frame needs to tell whether it wants the message at all.
    const mid = `${self}:${++count}`;
    const head = { kind: message.kind, name: message.name, key: message.key, id: message.id, to: message.to };
    const room = MAX_CHARS - JSON.stringify({ [SDK]: 1, kind: "part", mid, i: 0, n: 0, head, text: "" }).length - 24;
    const texts = slices(text, room);
    for (let i = 0; i < texts.length; i++) await w().broadcast({ [SDK]: 1, kind: "part", mid, i, n: texts.length, head, text: texts[i] });
  }

  /** Whether this frame would act on a message with these routing fields, so parts of one it would ignore aren't kept. */
  function wants({ kind, name, key, id, to }) {
    if (to !== undefined && to !== self) return false;
    if (kind === "ask") return answers.has(name);
    if (kind === "answer") return asked.has(id);
    if (kind === "tell") return events.has(name);
    if (kind === "state" || kind === "fresh" || kind === "put") return served.has(name) || shown.has(idOf(name, key));
    return true;
  }

  function receivePart({ mid, i, n, head, text }) {
    if (!head || !wants(head)) return;
    let p = partial.get(mid);
    if (!p) {
      p = { texts: new Array(n), got: 0, timer: setTimeout(() => partial.delete(mid), PART_TTL_MS) };
      partial.set(mid, p);
    }
    if (p.texts[i] === undefined) {
      p.texts[i] = text;
      p.got++;
    }
    if (p.got < n) return;
    clearTimeout(p.timer);
    partial.delete(mid);
    receive(JSON.parse(p.texts.join("")));
  }

  function forgetParts() {
    for (const p of partial.values()) clearTimeout(p.timer);
    partial.clear();
  }

  // ---------- asks and tells ----------

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
      // The asker hears why instead of waiting out its timeout.
      await send({ kind: "answer", id, error: `The answer to ${name} couldn't be sent: ${messageOf(error)}` });
    }
  }

  // ---------- queries in main ----------

  const focused = () => w().focused !== false;
  const watched = (e) => {
    if (e.local.size) return true;
    const now = Date.now();
    for (const [from, until] of e.leases) if (until <= now) e.leases.delete(from);
    return e.leases.size > 0;
  };
  const snapshot = (v) => ({ data: v.has ? v.data : null, error: v.error ?? null, at: v.at, loading: (!v.has && v.error === undefined) || v.waiting > 0 });

  /** As a new key comes in, forgets the ones nobody shows that weren't used for `DROP_MS`. */
  function sweep(s) {
    const now = Date.now();
    for (const e of [...s.entries.values()]) {
      if (watched(e) || e.loading || now - e.used < DROP_MS) continue;
      clearTimeout(e.refetch);
      clearTimeout(e.throttle);
      s.entries.delete(e.id);
    }
  }

  function entryOf(s, key) {
    const id = keyOf(key);
    let e = s.entries.get(id);
    if (!e) {
      sweep(s);
      e = { id, key: key ?? null, data: undefined, has: false, error: undefined, at: 0, landed: 0, used: 0, sent: undefined, loading: undefined, again: false, leases: new Map(), local: new Set() };
      s.entries.set(id, e);
    }
    e.used = Date.now();
    return e;
  }

  function stateOf(s, e, to) {
    const m = { kind: "state", name: s.name, key: e.key, at: e.at, to };
    if (e.has) m.data = e.data;
    if (e.error !== undefined) m.error = e.error;
    return m;
  }

  /** Reloads `refetchMs` after the value landed, while some part shows it and Wings is focused. */
  function schedule(s, e) {
    clearTimeout(e.refetch);
    e.refetch = undefined;
    // A key nobody shows waits for a page to show it, which loads it if it is old.
    if (!s.refetchMs || !s.load || !s.entries.has(e.id) || !watched(e)) return;
    e.refetch = setTimeout(() => {
      e.refetch = undefined;
      if (watched(e) && focused()) reload(s, e);
    }, Math.max(0, e.landed + s.refetchMs - Date.now()));
  }

  /** A new value for `e`, loaded or put at `at`: kept, and sent to every frame unless they all have it already. */
  function land(s, e, data, at, { broadcast = true } = {}) {
    e.data = data;
    e.has = true;
    e.at = at;
    e.error = undefined;
    e.landed = Date.now();
    const json = JSON.stringify(data);
    if (broadcast) {
      const m = json === e.sent ? { kind: "fresh", name: s.name, key: e.key, at } : stateOf(s, e);
      e.sent = json;
      send(m).catch((error) => {
        // Not sent, so the next value goes out even if it is the same.
        e.sent = undefined;
        console.error(error);
      });
    } else e.sent = json;
    deliver(e.local, snapshot(e));
    schedule(s, e);
  }

  function load(s, e) {
    if (e.loading) return e.loading;
    if (!s.load) return Promise.resolve(e.data);
    const at = Date.now();
    e.loading = (async () => s.load(e.key))().then(
      (data) => {
        e.loading = undefined;
        // A put that came after this load started is newer.
        if (at >= e.at) land(s, e, data ?? null, at);
        if (e.again) reload(s, e);
        return e.data;
      },
      (error) => {
        e.loading = undefined;
        // The last value stays, and every frame showing the key hears what went wrong.
        e.error = messageOf(error);
        e.landed = Date.now();
        send(stateOf(s, e)).catch(console.error);
        deliver(e.local, snapshot(e));
        schedule(s, e);
        if (e.again) reload(s, e);
        throw error;
      },
    );
    e.again = false;
    return e.loading;
  }

  const reload = (s, e) => void load(s, e).catch(() => {});

  /** An explicit refresh: after `throttleMs`, so a burst is one load, and once more after a load that is running. */
  function refreshEntry(s, e) {
    if (s.throttleMs) {
      e.throttle ??= setTimeout(() => {
        e.throttle = undefined;
        if (e.loading) e.again = true;
        else reload(s, e);
      }, s.throttleMs);
    } else if (e.loading) e.again = true;
    else reload(s, e);
  }

  function refreshServed(s, keyed, key) {
    if (keyed) return refreshEntry(s, entryOf(s, key));
    for (const e of [...s.entries.values()]) if (watched(e)) refreshEntry(s, e);
  }

  function onSub({ from, subs, renew }) {
    for (const [name, key] of subs) {
      const s = served.get(name);
      if (!s) continue;
      const e = entryOf(s, key);
      const live = (e.leases.get(from) ?? 0) > Date.now();
      e.leases.set(from, Date.now() + LEASE_MS);
      if (renew && live) continue;
      // A frame that just started showing the key: what main has, then a load if that is old.
      if (e.has || e.error !== undefined) send(stateOf(s, e, from)).catch(console.error);
      if (!e.has || Date.now() - e.landed >= s.staleMs) reload(s, e);
      else if (!e.refetch) schedule(s, e);
    }
  }

  function onUnsub({ from, subs }) {
    for (const [name, key] of subs) served.get(name)?.entries.get(keyOf(key))?.leases.delete(from);
  }

  function onPutServed(s, { key, data, at }) {
    const e = entryOf(s, key);
    if (at < e.at) return;
    // Every frame heard the put, so main has nothing to send.
    land(s, e, data, at, { broadcast: false });
  }

  function listenFocus() {
    if (focusHeard || !w().onFocus) return;
    focusHeard = true;
    // Back in Wings: the keys that missed a refetch while it was in the background load now.
    w().onFocus(({ focused: now }) => {
      if (!now) return;
      for (const s of served.values()) {
        if (!s.refetchMs || !s.load) continue;
        for (const e of s.entries.values()) if (watched(e) && !e.loading && Date.now() - e.landed >= s.refetchMs) reload(s, e);
      }
    });
  }

  function announce() {
    if (up) return;
    up = true;
    queueMicrotask(() => send({ kind: "up", boot: self }).catch(console.error));
  }

  // ---------- queries in a page ----------

  function flush() {
    const subs = [];
    const unsubs = [];
    for (const [id, what] of pending) (what === "sub" ? subs : unsubs).push(id);
    pending.clear();
    const pairs = (ids) => ids.map((id) => [id.slice(0, id.indexOf("\n")), JSON.parse(id.slice(id.indexOf("\n") + 1))]);
    if (unsubs.length) send({ kind: "unsub", from: self, subs: pairs(unsubs) }).catch(console.error);
    if (subs.length) send({ kind: "sub", from: self, subs: pairs(subs) }).catch(console.error);
  }

  function queue(id, what) {
    if (!pending.size) queueMicrotask(flush);
    pending.set(id, what);
  }

  const allShown = () => [...shown.values()].map((v) => [v.name, v.key]);

  function renew() {
    if (shown.size) send({ kind: "sub", from: self, subs: allShown(), renew: true }).catch(console.error);
  }

  function notify(v) {
    deliver(v.listeners, snapshot(v));
  }

  function onShownMessage(m) {
    const v = shown.get(idOf(m.name, m.key));
    if (!v || m.at < v.at) return;
    if (m.kind === "state" || m.kind === "put") {
      if ("data" in m) {
        v.data = m.data;
        v.has = true;
      }
      v.error = m.error;
    } else v.error = undefined;
    v.at = m.at;
    v.waiting = 0;
    notify(v);
  }

  // ---------- incoming ----------

  function receive(m) {
    if (m?.[SDK] !== 1) return;
    switch (m.kind) {
      case "ask":
        return void answerAsk(m).catch(console.error);
      case "answer":
        return settle(m.id, m.error, m.response);
      case "tell":
        return deliver(events.get(m.name), m.payload);
      case "part":
        return receivePart(m);
      case "up":
        // Main started, or started again: what was on its way is lost. It hears every key shown here, and sends what it
        // has for each one it hadn't heard of, so a page that subscribed before main, or before it restarted, gets its value.
        forgetParts();
        if (!served.size) renew();
        return;
      case "sub":
        return onSub(m);
      case "unsub":
        return onUnsub(m);
      case "refresh": {
        const s = served.get(m.name);
        if (s) refreshServed(s, "key" in m, m.key);
        return;
      }
      case "put": {
        const s = served.get(m.name);
        if (s) return onPutServed(s, m);
        return onShownMessage(m);
      }
      case "state":
      case "fresh":
        if (m.to !== undefined && m.to !== self) return;
        return onShownMessage(m);
    }
  }

  const listen = () => {
    if (listening) return;
    listening = true;
    w().onBroadcast(receive);
  };

  return {
    answer(name, handler) {
      listen();
      announce();
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

    serve(name, load, { refetchMs = 0, staleMs = 0, throttleMs = 0 } = {}) {
      listen();
      announce();
      if (refetchMs) listenFocus();
      served.set(name, { name, load, refetchMs, staleMs, throttleMs, entries: new Map() });
    },

    get(name, key, { maxAgeMs } = {}) {
      const s = served.get(name);
      if (!s) return Promise.reject(new Error(`${name} isn't served in this frame.`));
      const e = entryOf(s, key);
      if (e.has && (!s.load || Date.now() - e.landed < (maxAgeMs ?? s.staleMs))) return Promise.resolve(e.data);
      return load(s, e);
    },

    peek(name, key) {
      const e = served.get(name)?.entries.get(keyOf(key));
      return e && (e.has || e.error !== undefined) ? { data: e.has ? e.data : null, error: e.error ?? null, at: e.at } : undefined;
    },

    watching(name) {
      const s = served.get(name);
      return s ? [...s.entries.values()].filter(watched).map((e) => e.key) : [];
    },

    put(name, key, data) {
      const at = Date.now();
      const s = served.get(name);
      if (s) return void land(s, entryOf(s, key), data, at);
      listen();
      const v = shown.get(idOf(name, key));
      if (v && at >= v.at) {
        Object.assign(v, { data, has: true, error: undefined, at });
        notify(v);
      }
      send({ kind: "put", name, key, data, at, from: self }).catch(console.error);
    },

    refresh(name, ...rest) {
      const keyed = rest.length > 0;
      const s = served.get(name);
      if (s) return void refreshServed(s, keyed, rest[0]);
      listen();
      for (const v of shown.values()) {
        if (v.name !== name || (keyed && v.id !== idOf(name, rest[0]))) continue;
        v.waiting++;
        notify(v);
      }
      send(keyed ? { kind: "refresh", name, key: rest[0] ?? null, from: self } : { kind: "refresh", name, from: self }).catch(console.error);
    },

    subscribe(name, key, listener) {
      const s = served.get(name);
      if (s) {
        // Main showing its own query: no lease, a listener on the entry.
        const e = entryOf(s, key);
        e.local.add(listener);
        if (e.has || e.error !== undefined) listener(snapshot(e));
        if (!e.has || Date.now() - e.landed >= s.staleMs) reload(s, e);
        return () => void e.local.delete(listener);
      }
      listen();
      const id = idOf(name, key);
      let v = shown.get(id);
      if (!v) {
        v = { id, name, key: key ?? null, data: null, has: false, error: undefined, at: 0, waiting: 0, listeners: new Set() };
        shown.set(id, v);
        queue(id, "sub");
        renewing ??= pollWhileFocused(renew, RENEW_MS, { wings: w() });
      }
      v.listeners.add(listener);
      if (v.has || v.error !== undefined) listener(snapshot(v));
      return () => {
        if (!v.listeners.delete(listener) || v.listeners.size || shown.get(id) !== v) return;
        shown.delete(id);
        queue(id, "unsub");
        if (!shown.size) {
          renewing?.();
          renewing = undefined;
        }
      };
    },
  };
}
