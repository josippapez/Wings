// Small async helpers that plugins kept writing by hand.

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
 * The longest start of `rows` whose JSON is at most `max` characters, for a broadcast: Wings refuses one over 64,000,
 * counted by the length of its JSON text. With `wrap`, the JSON is of `wrap(rows)`, so the rest of the message counts
 * too: `fitJson(files, 64_000, (rows) => ({ type: "state", rows }))`.
 */
export function fitJson(rows, max, wrap = (all) => all) {
  const fits = (n) => JSON.stringify(wrap(rows.slice(0, n))).length <= max;
  if (fits(rows.length)) return rows.slice();
  // A longer start is always longer JSON, so the answer is found by halving.
  let lo = 0;
  let hi = rows.length - 1;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (fits(mid)) lo = mid;
    else hi = mid - 1;
  }
  return rows.slice(0, lo);
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
