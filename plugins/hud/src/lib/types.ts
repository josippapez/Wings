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

export interface Overview {
  fiveHour: Limit | null;
  sevenDay: Limit | null;
  statusline: boolean;
  memory: { used: number; total: number; percent: number } | null;
}

export interface SessionView {
  id: string;
  project: string;
  name: string | null;
  context: { percent: number; tokens: number; window: number; cache: string | null } | null;
  servers: Server[];
  skills: string[];
}

export interface Snapshot {
  overview: Overview;
  sessions: SessionView[];
}
