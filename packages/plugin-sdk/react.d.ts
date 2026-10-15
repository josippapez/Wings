/// <reference path="./wings.d.ts" />
import type { AskShape, Channel } from "./messages.js";
import type { Setting } from "./settings.js";

/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener: (message: unknown) => void): void;

export interface UseAskOptions {
  /** Asks again this often, in ms, while Wings is focused. */
  pollMs?: number;
  /** Asks nothing, for a request that isn't ready yet. `data` and `error` are `null` and `loading` is false. */
  skip?: boolean;
}

export interface AskResult<T> {
  /** The latest answer: kept while the next ask is on its way, `null` before the first one and once the request changes. */
  data: T | null;
  /** What the latest ask failed with, until the next answer. `data` keeps the answer before it. */
  error: Error | null;
  /** An ask is on its way. */
  loading: boolean;
  /** Asks again now, like a poll does. */
  refresh: () => void;
}

/**
 * Asks `name` on the channel when the component mounts and whenever `request` changes (compared as JSON), and every
 * `pollMs` while Wings is focused. An answer to an ask that a newer one has replaced is ignored.
 */
export function useAsk<A extends Record<keyof A, AskShape>, E extends object, S extends object, K extends keyof A & string>(
  ch: Channel<A, E, S>,
  name: K,
  request: A[K]["request"],
  options?: UseAskOptions,
): AskResult<A[K]["response"]>;

/** The state main shares as `name` on the channel, or `null` until main has sent it. */
export function useShared<A extends Record<keyof A, AskShape>, E extends object, S extends object, K extends keyof S & string>(ch: Channel<A, E, S>, name: K): S[K] | null;

/**
 * A setting's value, `null` until it is read, and a function that sets it. The value follows changes made in any frame
 * and `set` shows the new value at once.
 */
export function useSetting<T>(setting: Setting<T>): [T | null, (value: T) => Promise<void>];
