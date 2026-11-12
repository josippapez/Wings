/** What the main script shares with the sidebar. */

export type ServerState = "connected" | "failed" | "connecting" | "sign-in";

export interface Server {
  name: string;
  state: ServerState;
  tools?: number | null;
  error?: string;
  used: boolean;
  lastFailed: boolean;
}

export interface Limit {
  percent: number;
  resets: string | null;
  left: string | null;
  resetsAt: number | null;
}

/** One Claude Code account's limits. `account` is null for the default account. */
export interface AccountLimits {
  account: string | null;
  fiveHour: Limit | null;
  sevenDay: Limit | null;
}

export interface Overview {
  /** Newest first. */
  limits: AccountLimits[];
  statusline: boolean;
  memory: { used: number; total: number; percent: number } | null;
}

export interface SessionView {
  id: string;
  project: string;
  name: string | null;
  /** Its pane is the selected one. */
  focused: boolean;
  /** The Claude Code account it runs as, null for the default one. Left out until Claude Code has reported the session. */
  account?: string | null;
  context: { percent: number; tokens: number; window: number; cache: string | null } | null;
  servers: Server[];
  skills: string[];
}

export interface Snapshot {
  overview: Overview;
  sessions: SessionView[];
}
