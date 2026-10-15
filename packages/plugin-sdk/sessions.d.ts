/// <reference path="./wings.d.ts" />
/** A `claude` process in a pane, as the pane's `session` gives it. A resumed session is a new run, with a later `startedAt`. */
export type SessionRun = NonNullable<WingsPane["session"]>;

/** A pane that runs a Claude session. */
export type SessionPane = WingsPane & { session: SessionRun };

export interface FollowSessionsOptions<S> {
  /** Makes your object for a session, the first time a pane runs it. */
  create(sessionId: string): S;
  /** Each pane event while the session runs in `pane`: its `cwd`, `project` and the session's `name` and `state`. */
  pane?(session: S, pane: SessionPane): void;
  /** A new `claude` process runs the session: reset what you keep per run. */
  startRun(session: S, run: SessionRun): void;
  /** The run's older entries, oldest first. An entry that `onTranscript` sends too is passed on once. */
  backlog(session: S, run: SessionRun): Promise<TranscriptEntry[]>;
  /**
   * Each entry, oldest first: the backlog, then what Claude writes. `live` is false for the backlog. `thisRun` is
   * false for an entry written over 2 seconds before this `claude` started, which a resumed session's transcript also holds.
   */
  entry(session: S, entry: TranscriptEntry, info: { live: boolean; thisRun: boolean }): void;
  /** The session left `paneId`: the pane closed (`paneStillOpen` is false), or it runs something else now. */
  gone?(session: S, paneId: string, paneStillOpen: boolean): void;
  /** After anything above changed `session`, to batch updates. */
  changed?(session: S): void;
}

export interface FollowedSessions<S> {
  /** The sessions that run in a pane now. */
  sessions(): S[];
  get(sessionId: string): S | undefined;
  /**
   * Whether the session's current run has its backlog in, or its `backlog` failed. False while it loads, and for a
   * session no pane runs. `changed` is called when a run's backlog is in, so a view that waits for it draws then.
   */
  loaded(sessionId: string): boolean;
  /** Stops listening. Entries still loading are dropped. */
  stop(): void;
}

/**
 * Tracks the sessions `wings.onPanes` shows and calls your handlers, one run of one session at a time. A new `claude`
 * process (a new session, or a resumed one with a new `startedAt`) starts a run: `startRun`, then `backlog`, then
 * `entry` for each entry it returned. Entries Claude writes while the backlog loads wait for it, and an entry that is
 * in both is passed on once. Sidechain entries are skipped. A session no pane runs any more is dropped after `gone`.
 * A `startRun`, `backlog` or `entry` that throws is logged and the session carries on.
 */
export function followSessions<S>(options: FollowSessionsOptions<S>, w?: Pick<Wings, "onPanes" | "onTranscript">): FollowedSessions<S>;
