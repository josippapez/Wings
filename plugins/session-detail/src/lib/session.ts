import { basename, clip, oneLine } from "./format";
import { highlights, testOutcome } from "./highlights";
import type { Tag, Todo, TodoStatus } from "./types";

const KEEP = 150;
const EDITS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);

/** A row of the activity: a prompt you sent or a tool call. */
export interface Item {
  kind: "tool" | "prompt";
  key: string;
  summary: string;
  at: number;
  name?: string;
  tags?: Tag[] | null;
  outcome?: string | null;
  input?: [string, string][];
  result?: string | null;
  ms?: number | null;
  error?: boolean;
  done?: boolean;
  text?: string;
}

export interface ToolItem extends Item {
  kind: "tool";
  id: string;
  name: string;
  tags: Tag[] | null;
  outcome: string | null;
  input: [string, string][];
  result: string | null;
  ms: number | null;
  error: boolean;
  done: boolean;
}

interface Task {
  text: string;
  active: string | null;
  /** The assistant message that made the call. */
  messageId: string | undefined;
}

export interface Session {
  id: string;
  paneId: string | null;
  project: string;
  name: string | null;
  state: string;
  model: string | null;
  context: number | null;
  items: Item[];
  /** Tool calls still waiting for their result, by id. */
  tools: Map<string, ToolItem>;
  /** Edits by path. */
  files: Map<string, number>;
  todos: Todo[];
  tasks: Map<string, Todo>;
  /** `TaskCreate` calls waiting for the result that numbers them, by tool call id. */
  creating: Map<string, Task>;
  cwd: string | null;
  lastPrompt: string | null;
  updated: number;
  tokens: { input: number; output: number; cacheRead: number; cacheWrite: number };
  partial: boolean;
  loaded?: boolean;
  startedAt?: number;
}

/** The fields of a tool call's input this reads. */
interface ToolInput {
  command?: unknown;
  file_path?: string;
  notebook_path?: string;
  pattern?: unknown;
  url?: unknown;
  query?: unknown;
  description?: unknown;
  skill?: unknown;
  todos?: unknown;
  subject?: unknown;
  activeForm?: unknown;
  taskId?: unknown;
  status?: unknown;
}

interface Block {
  type?: string;
  id?: string;
  name?: string;
  input?: ToolInput | null;
  tool_use_id?: string;
  is_error?: boolean;
  content?: unknown;
  text?: string;
}

/** The parts of a Claude Code transcript entry this reads. */
export interface Entry {
  type: string;
  isSidechain?: boolean;
  isMeta?: boolean;
  timestamp?: string;
  message?: {
    id?: string;
    model?: string;
    usage?: { input_tokens?: number; output_tokens?: number; cache_creation_input_tokens?: number; cache_read_input_tokens?: number };
    content?: unknown;
  };
  attachment?: { type?: string; humanTurn?: boolean; prompt?: unknown };
}

/** By session id. */
export const sessions = new Map<string, Session>();

export function session(id: string) {
  let s = sessions.get(id);
  if (!s) {
    s = { id, paneId: null, project: "", name: null, state: "idle", model: null, context: null, items: [], tools: new Map(), files: new Map(), todos: [], tasks: new Map(), creating: new Map(), cwd: null, lastPrompt: null, updated: 0, tokens: emptyTokens(), partial: false };
    sessions.set(id, s);
  }
  return s;
}

export const emptyTokens = () => ({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 });
let nextKey = 1;

/** The fields of a tool call's input, each cut short, for the detail card. */
function inputFields(input: ToolInput | null = {}): [string, string][] {
  return Object.entries(input ?? {}).slice(0, 12).map(([key, value]) => [key, clip(typeof value === "string" ? value : JSON.stringify(value), 2000)]);
}
/** Text from a tool result or a prompt, which is a string or a list of content blocks. */
const textOf = (content: unknown) =>
  typeof content === "string" ? content : Array.isArray(content) ? (content as (Block | null)[]).filter((b) => b?.type === "text").map((b) => b!.text).join("\n") : "";

/** A few words on what a tool call does, from its input. */
function summary(name: string | undefined, input: ToolInput | null = {}) {
  // A tool call's input can be null in a transcript.
  input ??= {};
  if (name === "Bash") return oneLine(input.command);
  if (input.file_path || input.notebook_path) return basename(input.file_path ?? input.notebook_path);
  if (name === "Grep") return oneLine(input.pattern);
  if (name === "Glob") return oneLine(input.pattern);
  if (name === "WebFetch") return oneLine(input.url);
  if (name === "WebSearch") return oneLine(input.query);
  if (input.description) return oneLine(input.description);
  if (input.skill) return oneLine(input.skill);
  return "";
}

/** `mcp__server__tool` reads as `server · tool`. */
const toolName = (name: string) => (name.startsWith("mcp__") ? name.slice(5).split("__").join(" · ") : name);

/** Claude's own prompt wrappers, interrupt markers and the like aren't things you typed. */
const typed = (text: string) => text && !text.startsWith("<") && !text.startsWith("[Request interrupted");

function prompt(s: Session, text: string, at: number) {
  s.lastPrompt = oneLine(text, 200);
  s.items.push({ kind: "prompt", key: `prompt-${nextKey++}`, summary: oneLine(text, 140), text: clip(text, 4000), at });
}

const TODO_STATUS = new Set(["pending", "in_progress", "completed"]);
const MAX_TODOS = 40;

/** The checklist from `TodoWrite` (the whole list each time) and `TaskCreate`/`TaskUpdate` (one task at a time,
 * numbered by the result of the create). It reads earlier runs too, since the list carries over a resume. */
function trackTodos(s: Session, entry: Entry) {
  const content = entry.message?.content;
  if (!Array.isArray(content)) return;
  // Every result comes before Claude's next message, so a create still waiting when another message starts never gets one.
  // Claude Code writes one message as several entries with the same id.
  if (entry.type === "assistant") for (const [id, made] of s.creating) if (made.messageId !== entry.message?.id) s.creating.delete(id);
  for (const block of content as (Block | null)[]) {
    if (entry.type === "assistant" && block?.type === "tool_use") {
      const input = block.input ?? {};
      if (block.name === "TodoWrite" && Array.isArray(input.todos)) {
        const todos = input.todos as ({ content?: unknown; activeForm?: unknown; status?: unknown } | null)[];
        s.todos = todos
          .slice(0, MAX_TODOS)
          .filter((t) => t && typeof t.content === "string")
          .map((t) => ({ text: clip(t!.content as string, 200), active: typeof t!.activeForm === "string" ? clip(t!.activeForm, 200) : null, status: TODO_STATUS.has(t!.status as string) ? (t!.status as TodoStatus) : "pending" }));
      } else if (block.name === "TaskCreate" && typeof input.subject === "string") {
        s.creating.set(block.id!, { text: clip(input.subject, 200), active: typeof input.activeForm === "string" ? clip(input.activeForm, 200) : null, messageId: entry.message?.id });
      } else if (block.name === "TaskUpdate" && input.taskId != null) {
        const id = String(input.taskId);
        const task = s.tasks.get(id);
        if (input.status === "deleted") s.tasks.delete(id);
        else if (task) {
          if (TODO_STATUS.has(input.status as string)) task.status = input.status as TodoStatus;
          if (typeof input.subject === "string") task.text = clip(input.subject, 200);
          if (typeof input.activeForm === "string") task.active = clip(input.activeForm, 200);
        }
      }
    } else if (entry.type === "user" && block?.type === "tool_result" && s.creating.has(block.tool_use_id!)) {
      const made = s.creating.get(block.tool_use_id!)!;
      s.creating.delete(block.tool_use_id!);
      const id = /^Task #(\d+) created/.exec(textOf(block.content))?.[1];
      if (id && !block.is_error && s.tasks.size < MAX_TODOS) s.tasks.set(id, { text: made.text, active: made.active, status: "pending" });
    }
  }
}

/** Task tools win over `TodoWrite` while they have anything, since a session uses one or the other. */
export const checklist = (s: Session) => (s.tasks.size ? [...s.tasks.values()] : s.todos);

export function ingest(s: Session, entry: Entry) {
  if (entry.isSidechain) return;
  trackTodos(s, entry);
  const at = Date.parse(entry.timestamp as string) || Date.now();
  const message = entry.message ?? {};
  // A couple of seconds of slack, since the process start time is only to the second.
  const earlierRun = s.startedAt && at < s.startedAt - 2000;
  if (entry.type === "assistant") {
    if (message.model && message.model !== "<synthetic>") s.model = message.model;
    const u = message.usage;
    if (u) s.context = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
    if (earlierRun) return;
    if (u) {
      s.tokens.input += u.input_tokens ?? 0;
      s.tokens.output += u.output_tokens ?? 0;
      s.tokens.cacheRead += u.cache_read_input_tokens ?? 0;
      s.tokens.cacheWrite += u.cache_creation_input_tokens ?? 0;
    }
    for (const block of Array.isArray(message.content) ? (message.content as (Block | null)[]) : []) {
      if (block?.type !== "tool_use") continue;
      const marks = block.name === "Bash" ? highlights(block.input?.command) : null;
      const item: ToolItem = { kind: "tool", key: block.id!, id: block.id!, name: toolName(String(block.name)), summary: marks?.title ?? summary(block.name, block.input), tags: marks?.tags ?? null, outcome: null, input: inputFields(block.input), result: null, at, ms: null, error: false, done: false };
      s.tools.set(block.id!, item);
      s.items.push(item);
      const path = EDITS.has(block.name!) ? (block.input?.file_path ?? block.input?.notebook_path) : null;
      if (path) s.files.set(path, (s.files.get(path) ?? 0) + 1);
    }
  } else if (entry.type === "attachment" && !earlierRun) {
    // A message you send while Claude is working arrives as a queued command, not a user entry.
    const a = entry.attachment;
    if (a?.type === "queued_command" && a.humanTurn === true) {
      const text = textOf(a.prompt);
      if (typed(text)) prompt(s, text, at);
    }
  } else if (entry.type === "user" && !entry.isMeta && !earlierRun) {
    const content = message.content;
    if (typeof content === "string") {
      if (typed(content)) prompt(s, content, at);
      return;
    }
    for (const block of Array.isArray(content) ? (content as (Block | null)[]) : []) {
      if (block?.type !== "tool_result") continue;
      const item = s.tools.get(block.tool_use_id!);
      if (!item) continue;
      item.done = true;
      item.error = block.is_error === true;
      // A result stamped before its call has no duration to show.
      item.ms = at >= item.at ? at - item.at : null;
      const output = textOf(block.content);
      item.result = clip(output, 1500);
      if (item.tags?.some((t) => t.kind === "test")) item.outcome = testOutcome(output);
      s.tools.delete(block.tool_use_id!);
    }
  }
  if (s.items.length > KEEP) s.items.splice(0, s.items.length - KEEP);
  s.updated = Math.max(s.updated, at);
}
