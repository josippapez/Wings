/// <reference path="./wings.d.ts" />

/** One question: what the asker sends and what the answerer gives back. Both are JSON. */
export interface AskShape {
  request: unknown;
  response: unknown;
}

export interface ChannelOptions {
  /** The `wings` object to talk through. Tests pass a fake frame's; a plugin leaves it out. */
  wings?: Pick<Wings, "broadcast" | "onBroadcast"> & Partial<Pick<Wings, "focused" | "onFocus">>;
}

/** A payload argument, which can be left out when its type is `void`. */
type Payload<V> = [V] extends [void] ? [payload?: V] : [payload: V];

/** One query: the key a value is kept by, and the value. A query with a `void` key holds one value. Both are JSON. */
export interface QueryShape {
  key: unknown;
  value: unknown;
}

/** What a frame knows of one key of a query. */
export interface QuerySnapshot<V> {
  /** The latest value, `null` before the first one. */
  data: V | null;
  /** What the latest load failed with, until the next value. `data` keeps the value before it. */
  error: string | null;
  /** When the value was fetched, in ms since the epoch: the newest fetch wins. 0 before the first one. */
  at: number;
  /** No value or error yet, or a `refresh` from this frame is on its way. Main's own refetches never set it. */
  loading: boolean;
}

export interface ServeOptions {
  /** Main reloads a key this long after its last value landed, while a page shows it and Wings is focused. 0, the default, never. */
  refetchMs?: number;
  /** A page that starts showing a key gets the kept value, then a reload if that is older than this. Default 0: always. */
  staleMs?: number;
  /** Folds the refreshes asked for within this many ms into one load. Default 0. */
  throttleMs?: number;
}

/** A key argument, which can be left out when its type is `void`. */
type KeyArg<K> = [K] extends [void] ? [key?: K] : [key: K];

/**
 * What the plugin's frames say to each other, by name: `A` the questions, `E` the one-way events and `Q` the queries
 * main serves. The frames are its main script and its pages. A message reaches every other frame, never the one that
 * sent it, and goes over `wings.broadcast` as JSON. One too big for a broadcast goes in parts and is joined again.
 */
export interface Channel<A extends Record<keyof A, AskShape> = {}, E extends object = {}, Q extends Record<keyof Q, QueryShape> = {}> {
  /**
   * Answers `name` in this frame, usually main. A thrown error or a rejected promise reaches the asker as an `Error` with
   * the same message. One handler per name: a second call replaces the first. Returns a function that removes it.
   */
  answer<K extends keyof A & string>(name: K, handler: (request: A[K]["request"]) => A[K]["response"] | Promise<A[K]["response"]>): () => void;
  /**
   * Asks whichever frame answers `name`. Rejects with the answerer's error message; with "Wings didn't answer. Try
   * again." after `timeoutMs` (default 30 s); or with Wings' own message when it refuses the request.
   */
  ask<K extends keyof A & string>(name: K, ...args: [...Payload<A[K]["request"]>, options?: { timeoutMs?: number }]): Promise<A[K]["response"]>;
  /** Sends `payload` to the listeners of `name` in every other frame. Rejects when Wings refuses it. */
  tell<K extends keyof E & string>(name: K, ...payload: Payload<E[K]>): Promise<void>;
  /** Calls `listener` with each `payload` another frame tells `name`. Returns a function that removes it. */
  on<K extends keyof E & string>(name: K, listener: (payload: E[K]) => void): () => void;

  /**
   * Main: serves the query `name`. `load(key)` runs only here, one load per key at a time, and its value goes to every
   * frame showing the key. A failed load keeps the last value and sends its error to them. Without `load`, main keeps
   * what frames `put`. Pages that loaded before main, or while it reloaded, get the value once it serves.
   */
  serve<K extends keyof Q & string>(name: K, load?: (key: Q[K]["key"]) => Q[K]["value"] | Promise<Q[K]["value"]>, options?: ServeOptions): void;
  /** Main: the kept value when it landed at most `maxAgeMs` ago (default the query's `staleMs`), else one load, shared with any running. */
  get<K extends keyof Q & string>(name: K, key: Q[K]["key"], options?: { maxAgeMs?: number }): Promise<Q[K]["value"]>;
  /** Main: what is kept for a key, or `undefined`. Never loads. */
  peek<K extends keyof Q & string>(name: K, ...key: KeyArg<Q[K]["key"]>): Omit<QuerySnapshot<Q[K]["value"]>, "loading"> | undefined;
  /** Main: the keys of `name` some page, or main itself, shows now. */
  watching<K extends keyof Q & string>(name: K): Q[K]["key"][];
  /** Any frame: sets a key's value, in one broadcast to every frame. Main keeps it, and its next refetch moves on from it. */
  put<K extends keyof Q & string>(name: K, key: Q[K]["key"], value: Q[K]["value"]): void;
  /** Any frame: reloads `key` now, or with no key every key of `name` some frame shows. */
  refresh<K extends keyof Q & string>(name: K, key?: Q[K]["key"]): void;
  /**
   * Any frame: calls `listener` with the key's value as it changes, at once if this frame has one. A page keeps main told
   * while it listens. Returns a function that stops. React pages use `useQuery`.
   */
  subscribe<K extends keyof Q & string>(name: K, key: Q[K]["key"], listener: (snapshot: QuerySnapshot<Q[K]["value"]>) => void): () => void;
}

/**
 * A channel for main and the pages to share. Declare it once, in a module they all import, with the types of what it
 * carries:
 *
 * ```ts
 * export const ch = channel<
 *   { "pulls-action": { request: { path: string }; response: void } },
 *   { "tokens-changed": void },
 *   { pulls: { key: { path: string }; value: Pull[] }; hud: { key: void; value: HudState } }
 * >();
 * ```
 */
export function channel<A extends Record<keyof A, AskShape> = {}, E extends object = {}, Q extends Record<keyof Q, QueryShape> = {}>(options?: ChannelOptions): Channel<A, E, Q>;
