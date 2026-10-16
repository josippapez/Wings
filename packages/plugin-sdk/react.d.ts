/// <reference path="./wings.d.ts" />
import type { AskShape, Channel, QueryShape } from "./messages.js";
import type { Setting } from "./settings.js";

/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener: (message: unknown) => void): void;

export interface UseQueryOptions {
  /** Subscribes to nothing, for a key that isn't ready yet. `data` and `error` are `null` and `loading` is false. */
  skip?: boolean;
  /** Keeps the previous key's value while a new key loads, instead of `null`. */
  keepData?: boolean;
  /** With no value by then, `error` is "Wings didn't answer. Try again." Default 30 s. A later value clears it. */
  timeoutMs?: number;
}

export interface QueryResult<T> {
  /** The key's latest value, `null` before the first one. */
  data: T | null;
  /** What the latest load failed with, until the next value. `data` keeps the value before it. */
  error: Error | null;
  /** No value yet for this key, or this frame's `refresh()` waits. Main's own refetches never set it. */
  loading: boolean;
  /** When the value was fetched, in ms since the epoch. */
  at: number;
  /** Asks main to load the key again now. */
  refresh: () => void;
}

/**
 * The value of the query `name` for `key`, which main serves on the channel, and its updates. Components in one frame
 * showing one key share one subscription, and every frame showing it gets the same value from one broadcast.
 */
export function useQuery<A extends Record<keyof A, AskShape>, E extends object, Q extends Record<keyof Q, QueryShape>, K extends keyof Q & string>(
  ch: Channel<A, E, Q>,
  name: K,
  key: Q[K]["key"],
  options?: UseQueryOptions,
): QueryResult<Q[K]["value"]>;

/**
 * A setting's value, `null` until it is read, and a function that sets it. The value follows changes made in any frame
 * and `set` shows the new value at once.
 */
export function useSetting<T>(setting: Setting<T>): [T | null, (value: T) => Promise<void>];
