/// <reference path="./wings.d.ts" />
/** The message of an Error, or the text of anything else that was thrown. */
export function errorMessage(error: unknown): string;

export interface CachedOptions<K> {
  /** How long a value is kept after its load finished, in ms. For good when unset. */
  ttlMs?: number;
  /** Remember a failure this long, in ms, and fail the same way meanwhile. Failures aren't kept when unset. */
  failTtlMs?: number;
  /** Turns the argument of `load` into the key. `String` when unset, which only suits a string. */
  key?: (key: K) => string;
}

export interface Cached<K, V> {
  /** The kept value, else the load already running, else a new load. `fresh` skips a kept value but joins a running load. */
  get(key: K, options?: { fresh?: boolean }): Promise<V>;
  /** The last value loaded for `key`, whatever its age, or `undefined`. Never loads. */
  peek(key: K): V | undefined;
  /** Forgets `key`, a running load too, so the next `get` loads again. */
  delete(key: K): void;
  clear(): void;
}

/**
 * Loads a value per key and keeps it, so `load` runs once however many ask. `get(key)` gives the kept value, else the
 * load already running, else starts one. A value is kept for `ttlMs` after its load finished, for good when unset. A
 * failure isn't kept, so the next `get` tries again, unless `failTtlMs` is set: then `get` fails the same way for that
 * long without loading. `fresh` skips a kept value but still joins a load that is running. `peek` gives the last value
 * whatever its age and never loads. `delete` and `clear` forget keys, running loads included. `key` turns the argument
 * of `load` into a string when it isn't one.
 */
export function cached<V>(load: (key: string) => V | Promise<V>, options?: CachedOptions<string>): Cached<string, V>;
export function cached<K, V>(load: (key: K) => V | Promise<V>, options: CachedOptions<K> & { key: (key: K) => string }): Cached<K, V>;

/**
 * Returns a function that runs the jobs it is given one at a time, in the order they came, and gives each job's own
 * result. A job that fails doesn't stop the ones after it.
 */
export function serial(): <T>(job: () => T | Promise<T>) => Promise<T>;

/**
 * Returns `fn` made to run once at a time. Calling it while `fn` runs doesn't start a second run: it makes `fn` run
 * once more when this one is done, however many calls came meanwhile. It takes no arguments, so a plugin that needs to
 * say what the rerun is for keeps a flag. The promise settles when the run it joined, and any rerun, is done, and
 * rejects if `fn` does, dropping the rerun.
 */
export function singleFlight(fn: () => unknown): () => Promise<void>;

/**
 * Runs `fn(item, index)` over `items`, at most `limit` at a time, and resolves the results in the order of `items`. When
 * one fails, no further item starts and the call rejects with that error.
 */
export function mapLimit<T, R>(items: readonly T[], limit: number, fn: (item: T, index: number) => R | Promise<R>): Promise<R[]>;

/**
 * For a load that can be started again before the last one answered, like a refresh while one is running. Returns a
 * function to call as a load starts; it gives that load's `stale()`, true once a later load has started. Check it after
 * each `await`, in `catch` and in `finally`, and drop the answer when it's true.
 */
export function latestOnly(): () => () => boolean;

/**
 * The longest start of `rows` whose JSON is at most `max` characters, for a broadcast: Wings refuses one over 64,000,
 * counted by the length of its JSON text. With `wrap`, the JSON is of `wrap(rows)`, so the rest of the message counts
 * too: `fitJson(files, 64_000, (rows) => ({ type: "state", rows }))`.
 */
export function fitJson<T>(rows: readonly T[], max: number, wrap?: (rows: T[]) => unknown): T[];

/** What `wings.openDiff` takes, without a patch: that is what `openDiffWhile` loads. */
export type DiffOpen = Omit<Parameters<Wings["openDiff"]>[0], "patch">;
/** What `wings.updateDiff` takes for a viewer that is loading. */
export type DiffUpdate = Parameters<Wings["updateDiff"]>[1];

/**
 * Opens the diff viewer at once, loading, then fills it with what `load(id)` resolves: `{ patch }` or `{ error }`, with
 * `comments` and `focus` if it has them. A `load` that throws shows its message in the viewer. `id` is the viewer's, for
 * a plugin that keeps track of its viewers. Resolves when the viewer has been updated.
 */
export function openDiffWhile(diff: DiffOpen, load: (id: string) => DiffUpdate | Promise<DiffUpdate>): Promise<void>;
