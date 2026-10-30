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
  /** The value a finished load gave for `key`, whatever its age. `undefined` while it loads, after a failure or once forgotten. Never loads. */
  peek(key: K): V | undefined;
  /** Forgets `key`, a running load too, so the next `get` loads again. */
  delete(key: K): void;
  clear(): void;
}

/**
 * Loads a value per key and keeps it, so `load` runs once however many ask. `get(key)` gives the kept value, else the
 * load already running, else starts one. A value is kept for `ttlMs` after its load finished, for good when unset. A
 * failure isn't kept, so the next `get` tries again, unless `failTtlMs` is set: then `get` fails the same way for that
 * long without loading. `fresh` skips a kept value but still joins a load that is running. `peek` gives the value a
 * finished load gave, whatever its age, and nothing while the key is loading, after a failure or once forgotten; it
 * never loads. `delete` and `clear` forget keys, running loads included. `key` turns the argument of `load` into a
 * string when it isn't one.
 */
export function cached<V>(load: (key: string) => V | Promise<V>, options?: CachedOptions<string>): Cached<string, V>;
export function cached<K, V>(load: (key: K) => V | Promise<V>, options: CachedOptions<K> & { key: (key: K) => string }): Cached<K, V>;

export interface BatchedOptions<K> {
  /** The most args one `loadMany` call takes. A larger batch is split. No limit when unset. */
  max?: number;
  /** Turns an arg into the key that equal args share a slot by. `String` when unset, which only suits a string or a number. */
  key?: (arg: K) => string;
}

export interface Batched<K, V> {
  /** The value for `arg`, from the `loadMany` call of every `get` made in this microtask turn. */
  get(arg: K): Promise<V>;
}

/**
 * Turns many loads of one thing into one call of `loadMany`. Every `get(arg)` made in the same microtask turn (a
 * `Promise.all(items.map(...))` whose callbacks don't await anything first) goes out together as one `loadMany(args)`,
 * which resolves one value per arg in the same order, split into calls of at most `max` args. Equal keys (`key` turns
 * an arg into a string, as in `cached`) share one slot. A `loadMany` that fails rejects every `get` of its call, and a
 * value that is an `Error` rejects only its own `get`. It keeps nothing: put `cached(batch.get, { ttlMs })` over it
 * for a TTL.
 */
export function batched<K, V>(loadMany: (args: K[]) => (V | Error)[] | Promise<(V | Error)[]>, options?: BatchedOptions<K>): Batched<K, V>;

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
