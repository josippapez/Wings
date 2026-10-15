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

/** A row of the activity: a prompt you sent or a tool call. A prompt has only `kind`, `key`, `summary`, `at` and `text`. */
export interface Item {
  kind: "tool" | "prompt";
  key: string;
  summary: string;
  at: number;
  name?: string;
  tags?: Tag[] | null;
  outcome?: string | null;
  ms?: number | null;
  error?: boolean;
  done?: boolean;
  /** What the row opens to: a tool call's inputs and the start of its result, or the prompt in full. */
  input?: [string, string][];
  result?: string | null;
  text?: string;
}

/** A row as the sidebar lists it. What it opens to is left out, and asked for when you open the row. */
export type ItemView = Omit<Item, "input" | "result" | "text">;

export interface SessionView {
  id: string;
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
