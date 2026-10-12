/** What the main script and the sidebar send each other. */

export interface Tag {
  kind: string;
  label: string;
}

export type TodoStatus = "pending" | "in_progress" | "completed";

export interface Todo {
  text: string;
  active: string | null;
  status: TodoStatus;
}

/** A row of the activity. A prompt has only `kind`, `key`, `summary` and `at`. */
export interface ItemView {
  kind: "tool" | "prompt";
  key: string;
  name?: string;
  summary: string;
  tags?: Tag[] | null;
  outcome?: string | null;
  at: number;
  ms?: number | null;
  error?: boolean;
  done?: boolean;
}

export interface SessionView {
  id: string;
  paneId: string | null;
  project: string;
  name: string | null;
  state: string;
  model: string;
  context: number | null;
  tokens: string;
  running: { name: string; summary: string; at: number }[];
  items: ItemView[];
  todos: Todo[];
  /** Path and how many edits, most edited first. */
  files: [string, number][];
}

/** What a row opens to. A prompt has `text`; a tool call has the rest. */
export interface Detail {
  kind: "tool" | "prompt";
  name?: string;
  at: number;
  ms?: number | null;
  error?: boolean;
  done?: boolean;
  input?: [string, string][];
  result?: string | null;
  text?: string;
}

/** From the main script. */
export type ToPage = { type: "sessions"; sessions: SessionView[] } | { type: "detail"; sessionId: string; key: string; detail: Detail };

/** From the sidebar. */
export type ToMain = { type: "hello" } | { type: "diff"; sessionId: string; path: string | null } | { type: "detail"; sessionId: string; key: string };
