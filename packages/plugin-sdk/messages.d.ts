/// <reference path="./wings.d.ts" />

/** One question: what the asker sends and what the answerer gives back. Both are JSON. */
export interface AskShape {
  request: unknown;
  response: unknown;
}

export interface ChannelOptions {
  /** The `wings` object to talk through. Tests pass a fake frame's; a plugin leaves it out. */
  wings?: Pick<Wings, "broadcast" | "onBroadcast">;
}

/** A payload argument, which can be left out when its type is `void`. */
type Payload<V> = [V] extends [void] ? [payload?: V] : [payload: V];

/**
 * What the plugin's frames say to each other, by name: `A` the questions, `E` the one-way events and `S` the states
 * main shares. The frames are its main script and its pages. A message reaches every other frame, never the one that
 * sent it, and goes over `wings.broadcast`, so it is JSON and up to 64 KB.
 */
export interface Channel<A extends Record<keyof A, AskShape> = {}, E extends object = {}, S extends object = {}> {
  /**
   * Answers `name` in this frame, usually main. A thrown error or a rejected promise reaches the asker as an `Error` with
   * the same message. One handler per name: a second call replaces the first. Returns a function that removes it.
   */
  answer<K extends keyof A & string>(name: K, handler: (request: A[K]["request"]) => A[K]["response"] | Promise<A[K]["response"]>): () => void;
  /**
   * Asks whichever frame answers `name`. Rejects with the answerer's error message; with "Wings didn't answer. Try
   * again." after `timeoutMs` (default 30 s); or with Wings' own message when it refuses the request, like one over 64 KB.
   */
  ask<K extends keyof A & string>(name: K, ...args: [...Payload<A[K]["request"]>, options?: { timeoutMs?: number }]): Promise<A[K]["response"]>;
  /** Sends `payload` to the listeners of `name` in every other frame. Rejects when Wings refuses it, like one over 64 KB. */
  tell<K extends keyof E & string>(name: K, ...payload: Payload<E[K]>): Promise<void>;
  /** Calls `listener` with each `payload` another frame tells `name`. Returns a function that removes it. */
  on<K extends keyof E & string>(name: K, listener: (payload: E[K]) => void): () => void;
  /**
   * Publishes the state `name`, built by `snapshot()` (JSON, under 64 KB), usually from main. Call `changed()` whenever
   * it may have changed. One update goes out `throttleMs` (default 300) after the first change, so a burst is one send,
   * and nothing goes out when the state is the same as the last one sent. A page that opens later asks for the state
   * and gets it, with no code in main. A state Wings refuses is logged.
   */
  share<K extends keyof S & string>(name: K, snapshot: () => S[K], options?: { throttleMs?: number }): { changed(): void };
  /**
   * Calls `listener` with the state main shares as `name`: once it answers the request this makes for the current state,
   * and again on each change. Returns a function that stops.
   */
  watch<K extends keyof S & string>(name: K, listener: (state: S[K]) => void): () => void;
}

/**
 * A channel for main and the pages to share. Declare it once, in a module they all import, with the types of what it
 * carries:
 *
 * ```ts
 * export const ch = channel<
 *   { pulls: { request: { path: string }; response: Pull[] } },
 *   { "pulls-changed": void },
 *   { hud: HudState }
 * >();
 * ```
 */
export function channel<A extends Record<keyof A, AskShape> = {}, E extends object = {}, S extends object = {}>(options?: ChannelOptions): Channel<A, E, S>;
