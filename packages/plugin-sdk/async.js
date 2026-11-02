// Small async helpers that plugins kept writing by hand.

import { pollWhileFocused } from "./poll.js";

/** The message of an Error, or the text of anything else that was thrown. */
export const errorMessage = (error) => String(error?.message ?? error);

/**
 * Loads a value per key and keeps it, so `load` runs once however many ask. `get(key)` gives the kept value, else the
 * load already running, else starts one. A value is kept for `ttlMs` after its load finished, for good when unset. A
 * failure isn't kept, so the next `get` tries again, unless `failTtlMs` is set: then `get` fails the same way for that
 * long without loading. `fresh` skips a kept value but still joins a load that is running. `peek` gives the value a
 * finished load gave, whatever its age, and nothing while the key is loading, after a failure or once forgotten; it
 * never loads. `delete` and `clear` forget keys, running loads included. `key` turns the argument of `load` into a
 * string when it isn't one.
 */
export function cached(load, { ttlMs = Infinity, failTtlMs = 0, key = String } = {}) {
  /** key → the load, and once it finished: when, whether it failed, and its value. */
  const entries = new Map();
  const kept = (e) => e.at !== undefined && Date.now() - e.at < (e.failed ? failTtlMs : ttlMs);
  return {
    get(arg, { fresh = false } = {}) {
      const id = key(arg);
      const hit = entries.get(id);
      if (hit && (hit.at === undefined || (!fresh && kept(hit)))) return hit.promise;
      const entry = { promise: (async () => load(arg))(), at: undefined, failed: false, value: undefined };
      entries.set(id, entry);
      entry.promise.then(
        (value) => {
          entry.value = value;
          entry.at = Date.now();
        },
        () => {
          if (failTtlMs > 0) Object.assign(entry, { failed: true, at: Date.now() });
          else if (entries.get(id) === entry) entries.delete(id);
        },
      );
      return entry.promise;
    },
    peek: (arg) => entries.get(key(arg))?.value,
    delete: (arg) => void entries.delete(key(arg)),
    clear: () => entries.clear(),
  };
}

/**
 * Turns many loads of one thing into one call of `loadMany`. Every `get(arg)` made in the same microtask turn (a
 * `Promise.all(items.map(...))` whose callbacks don't await anything first) goes out together as one `loadMany(args)`,
 * which resolves one value per arg in the same order, split into calls of at most `max` args. Equal keys (`key` turns
 * an arg into a string, as in `cached`) share one slot. A `loadMany` that fails rejects every `get` of its call, and a
 * value that is an `Error` rejects only its own `get`. It keeps nothing: put `cached(batch.get, { ttlMs })` over it
 * for a TTL.
 */
export function batched(loadMany, { max = Infinity, key = String } = {}) {
  /** key → the slot of a `get` that hasn't gone out yet. */
  let queue = new Map();
  const run = async (slots) => {
    try {
      const values = await loadMany(slots.map((s) => s.arg));
      if (!Array.isArray(values) || values.length !== slots.length) throw new Error(`loadMany gave ${Array.isArray(values) ? values.length : "no list of"} values for ${slots.length} args`);
      slots.forEach((s, i) => (values[i] instanceof Error ? s.reject(values[i]) : s.resolve(values[i])));
    } catch (error) {
      slots.forEach((s) => s.reject(error));
    }
  };
  const flush = () => {
    const slots = [...queue.values()];
    queue = new Map();
    for (let i = 0; i < slots.length; i += max) void run(slots.slice(i, i + max));
  };
  return {
    get(arg) {
      const id = key(arg);
      let slot = queue.get(id);
      if (!slot) {
        slot = { arg };
        slot.promise = new Promise((resolve, reject) => Object.assign(slot, { resolve, reject }));
        if (!queue.size) queueMicrotask(flush);
        queue.set(id, slot);
      }
      return slot.promise;
    },
  };
}

/**
 * Returns a function that runs the jobs it is given one at a time, in the order they came, and gives each job's own
 * result. A job that fails doesn't stop the ones after it.
 */
export function serial() {
  let tail = Promise.resolve();
  return (job) => {
    const run = tail.then(() => job());
    tail = run.catch(() => {});
    return run;
  };
}

/**
 * Returns `fn` made to run once at a time. Calling it while `fn` runs doesn't start a second run: it makes `fn` run
 * once more when this one is done, however many calls came meanwhile. It takes no arguments, so a plugin that needs to
 * say what the rerun is for keeps a flag. The promise settles when the run it joined, and any rerun, is done, and
 * rejects if `fn` does, dropping the rerun.
 */
export function singleFlight(fn) {
  let running = false;
  let again = false;
  let current;
  return () => {
    if (running) {
      again = true;
      return current;
    }
    running = true;
    current = (async () => {
      try {
        do {
          again = false;
          await fn();
        } while (again);
      } finally {
        running = false;
      }
    })();
    return current;
  };
}

/**
 * Runs `fn(item, index)` over `items`, at most `limit` at a time, and resolves the results in the order of `items`. When
 * one fails, no further item starts and the call rejects with that error.
 */
export async function mapLimit(items, limit, fn) {
  const out = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      try {
        out[i] = await fn(items[i], i);
      } catch (error) {
        next = items.length;
        throw error;
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return out;
}

/**
 * For a load that can be started again before the last one answered, like a refresh while one is running. Returns a
 * function to call as a load starts; it gives that load's `stale()`, true once a later load has started. Check it after
 * each `await`, in `catch` and in `finally`, and drop the answer when it's true.
 */
export function latestOnly() {
  let latest = 0;
  return () => {
    const mine = ++latest;
    return () => mine !== latest;
  };
}

/**
 * Opens the diff viewer at once, loading, then fills it with what `load(id)` resolves: `{ patch }` or `{ error }`, with
 * `comments` and `focus` if it has them. A `load` that throws shows its message in the viewer. `id` is the viewer's, for
 * a plugin that keeps track of its viewers. Resolves when the viewer has been updated.
 */
export async function openDiffWhile(diff, load) {
  const { id } = await globalThis.wings.openDiff(diff);
  let update;
  try {
    update = await load(id);
  } catch (error) {
    update = { error: errorMessage(error) };
  }
  await globalThis.wings.updateDiff(id, update);
}

/** How often an open `openLiveDiff` viewer loads again while Wings is in front. */
const LIVE_DIFF_MS = 15_000;
/** The viewers `openLiveDiff` keeps up to date, by id: their load, key, the update they show as JSON, and their queue. */
const liveDiffs = new Map();
let stopLivePoll = null;
let liveHooked = false;

/** Runs a viewer's load and sends the update when it differs from what the viewer shows. One load at a time per viewer. */
function reloadLiveDiff(id, viewer, fresh) {
  return viewer.queue(async () => {
    let update;
    try {
      update = await viewer.load({ fresh });
    } catch (error) {
      update = { error: errorMessage(error) };
    }
    const shown = JSON.stringify(update);
    if (!liveDiffs.has(id) || shown === viewer.shown) return;
    viewer.shown = shown;
    await globalThis.wings.updateDiff(id, update);
  });
}

/**
 * Like `openDiffWhile`, for a diff that can change while it's open, like the working tree or a pull request. Needs Wings
 * 0.1.113, from the main script. The viewer gets a Refresh button, and `load` runs again every 15 s while Wings is in
 * front. An update the viewer already shows isn't sent again, and nothing runs once it's closed. `load({ fresh })`:
 * `fresh` is true for the Refresh button, to skip a cache. With `patch` in `diff` the viewer opens filled and `load`
 * waits for the first refresh. `key` names what it shows, like a repo folder, for `refreshLiveDiffs`. It takes over
 * `wings.onDiffRefresh`. Resolves the viewer's `{ id }` once it shows its first update.
 */
export async function openLiveDiff(diff, load, { key } = {}) {
  const w = globalThis.wings;
  if (!liveHooked) {
    liveHooked = true;
    w.onDiffRefresh(({ diffId }) => {
      const viewer = liveDiffs.get(diffId);
      return viewer && reloadLiveDiff(diffId, viewer, true);
    });
    w.onDiffClosed(({ diffId }) => {
      liveDiffs.delete(diffId);
      if (liveDiffs.size > 0 || !stopLivePoll) return;
      stopLivePoll();
      stopLivePoll = null;
    });
  }
  const { id } = await w.openDiff({ ...diff, refresh: true });
  const viewer = { load, key, shown: diff.patch === undefined ? undefined : JSON.stringify({ patch: diff.patch, comments: diff.comments }), queue: serial() };
  liveDiffs.set(id, viewer);
  stopLivePoll ??= pollWhileFocused(() => void refreshLiveDiffs(), LIVE_DIFF_MS);
  if (viewer.shown === undefined) await reloadLiveDiff(id, viewer, false);
  return { id };
}

/** Loads the viewers `openLiveDiff` opened again now, or only those whose `key` is in `keys`, like after a git change. */
export function refreshLiveDiffs(keys) {
  return Promise.all([...liveDiffs].filter(([, v]) => !keys || keys.includes(v.key)).map(([id, v]) => reloadLiveDiff(id, v, false))).then(() => {});
}
