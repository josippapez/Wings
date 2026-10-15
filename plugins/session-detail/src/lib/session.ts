import { basename, truncate } from "wings-plugin-sdk/format";

import { oneLine } from "./format";
import { highlights, testOutcome } from "./highlights";
import type { Item, Tag, Todo, TodoStatus } from "./types";

const KEEP = 150;
const EDITS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);

/** A tool call, whose result is filled in when it arrives. */
export interface ToolItem extends Item {
  kind: "tool";
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

/** What one `claude` process did. A new process, even for the same session after a resume, starts the activity afresh. */
interface Run {
  items: Item[];
  /** Tool calls still waiting for their result, by id. */
  tools: Map<string, ToolItem>;
  /** Edits by path. */
  files: Map<string, number>;
  todos: Todo[];
  tasks: Map<string, Todo>;
  /** `TaskCreate` calls waiting for the result that numbers them, by tool call id. */
  creating: Map<string, Task>;
  tokens: { input: number; output: number; cacheRead: number; cacheWrite: number };
  partial: boolean;
}

export interface Session extends Run {
  id: string;
  project: string;
  name: string | null;
  state: string;
  model: string | null;
  context: number | null;
  cwd: string | null;
  updated: number;
}

export const fresh = (): Run => ({ items: [], tools: new Map(), files: new Map(), todos: [], tasks: new Map(), creating: new Map(), tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, partial: false });

export const session = (id: string): Session => ({ id, project: "", name: null, state: "idle", model: null, context: null, cwd: null, updated: 0, ...fresh() });

let nextKey = 1;

/** A message's content blocks. A transcript can hold a null among them, so each is checked. */
const blocksOf = (message: TranscriptMessage | undefined): (TranscriptBlock | null)[] => (Array.isArray(message?.content) ? message.content : []);

/** The fields of a tool call's input, each cut short, for the detail card. */
function inputFields(input: TranscriptBlock["input"]): [string, string][] {
  return Object.entries(input ?? {}).slice(0, 12).map(([key, value]) => [key, truncate(typeof value === "string" ? value : JSON.stringify(value), 2000)]);
}
/** Text from a tool result or a prompt, which is a string or a list of content blocks. */
const textOf = (content: unknown) =>
  typeof content === "string" ? content : Array.isArray(content) ? (content as (TranscriptBlock | null)[]).filter((b) => b?.type === "text").map((b) => b!.text).join("\n") : "";

/** A few words on what a tool call does, from its input. */
function summary(name: string | undefined, input: TranscriptBlock["input"]) {
  // A tool call's input can be null in a transcript.
  input ??= {};
  if (name === "Bash") return oneLine(input.command);
  if (input.file_path || input.notebook_path) return basename(String(input.file_path ?? input.notebook_path));
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
  s.items.push({ kind: "prompt", key: `prompt-${nextKey++}`, summary: oneLine(text, 140), text: truncate(text, 4000), at });
}

const TODO_STATUS = new Set(["pending", "in_progress", "completed"]);
const MAX_TODOS = 40;

/** The checklist from `TodoWrite` (the whole list each time) and `TaskCreate`/`TaskUpdate` (one task at a time,
 * numbered by the result of the create). It reads earlier runs too, since the list carries over a resume. */
function trackTodos(s: Session, entry: TranscriptEntry) {
  if (entry.type !== "assistant" && entry.type !== "user") return;
  const content = entry.message?.content;
  if (!Array.isArray(content)) return;
  // Every result comes before Claude's next message, so a create still waiting when another message starts never gets one.
  // Claude Code writes one message as several entries with the same id.
  if (entry.type === "assistant") for (const [id, made] of s.creating) if (made.messageId !== entry.message?.id) s.creating.delete(id);
  const blocks: (TranscriptBlock | null)[] = content;
  for (const block of blocks) {
    if (entry.type === "assistant" && block?.type === "tool_use") {
      const input = block.input ?? {};
      if (block.name === "TodoWrite" && Array.isArray(input.todos)) {
        const todos = input.todos as ({ content?: unknown; activeForm?: unknown; status?: unknown } | null)[];
        s.todos = todos
          .slice(0, MAX_TODOS)
          .filter((t) => t && typeof t.content === "string")
          .map((t) => ({ text: truncate(t!.content as string, 200), active: typeof t!.activeForm === "string" ? truncate(t!.activeForm, 200) : null, status: TODO_STATUS.has(t!.status as string) ? (t!.status as TodoStatus) : "pending" }));
      } else if (block.name === "TaskCreate" && typeof input.subject === "string") {
        s.creating.set(block.id!, { text: truncate(input.subject, 200), active: typeof input.activeForm === "string" ? truncate(input.activeForm, 200) : null, messageId: entry.message?.id });
      } else if (block.name === "TaskUpdate" && input.taskId != null) {
        const id = String(input.taskId);
        const task = s.tasks.get(id);
        if (input.status === "deleted") s.tasks.delete(id);
        else if (task) {
          if (TODO_STATUS.has(input.status as string)) task.status = input.status as TodoStatus;
          if (typeof input.subject === "string") task.text = truncate(input.subject, 200);
          if (typeof input.activeForm === "string") task.active = truncate(input.activeForm, 200);
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

/**
 * Adds an entry to the session. `thisRun` is false for one written before this `claude` started, which a resumed
 * session's transcript also holds: those count for the model and context, and for the to-do list.
 */
export function ingest(s: Session, entry: TranscriptEntry, thisRun: boolean) {
  trackTodos(s, entry);
  const at = Date.parse(entry.timestamp ?? "") || Date.now();
  if (entry.type === "assistant") {
    const message = entry.message ?? {};
    if (message.model && message.model !== "<synthetic>") s.model = message.model;
    const u = message.usage;
    if (u) s.context = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
    if (!thisRun) return;
    if (u) {
      s.tokens.input += u.input_tokens ?? 0;
      s.tokens.output += u.output_tokens ?? 0;
      s.tokens.cacheRead += u.cache_read_input_tokens ?? 0;
      s.tokens.cacheWrite += u.cache_creation_input_tokens ?? 0;
    }
    for (const block of blocksOf(message)) {
      if (block?.type !== "tool_use") continue;
      const marks = block.name === "Bash" ? highlights(block.input?.command) : null;
      const item: ToolItem = { kind: "tool", key: block.id!, name: toolName(String(block.name)), summary: marks?.title ?? summary(block.name, block.input), tags: marks?.tags ?? null, outcome: null, input: inputFields(block.input), result: null, at, ms: null, error: false, done: false };
      s.tools.set(block.id!, item);
      s.items.push(item);
      const path = EDITS.has(block.name!) ? (block.input?.file_path ?? block.input?.notebook_path) : null;
      if (typeof path === "string" && path) s.files.set(path, (s.files.get(path) ?? 0) + 1);
    }
  } else if (entry.type === "attachment" && thisRun) {
    // A message you send while Claude is working arrives as a queued command, not a user entry.
    const a = entry.attachment;
    if (a?.type === "queued_command" && a.humanTurn === true) {
      const text = textOf(a.prompt);
      if (typed(text)) prompt(s, text, at);
    }
  } else if (entry.type === "user" && !entry.isMeta && thisRun) {
    const content = entry.message?.content;
    if (typeof content === "string") {
      if (typed(content)) prompt(s, content, at);
      return;
    }
    for (const block of blocksOf(entry.message)) {
      if (block?.type !== "tool_result") continue;
      const item = s.tools.get(block.tool_use_id!);
      if (!item) continue;
      item.done = true;
      item.error = block.is_error === true;
      // A result stamped before its call has no duration to show.
      item.ms = at >= item.at ? at - item.at : null;
      const output = textOf(block.content);
      item.result = truncate(output, 1500);
      if (item.tags?.some((t) => t.kind === "test")) item.outcome = testOutcome(output);
      s.tools.delete(block.tool_use_id!);
    }
  }
  if (s.items.length > KEEP) s.items.splice(0, s.items.length - KEEP);
  s.updated = Math.max(s.updated, at);
}
