// The tools Claude calls: what each accepts, checked before anything runs.

import { isPriority, isStatus, isType, LIMITS, MAX_LINKS, normalizeKey, PRIORITIES, STATUSES, TYPES, type IssueType, type Op, type Priority, type Status } from "./domain";

export type ToolCall = { tool: "list"; status?: Status; type?: IssueType; query?: string } | { tool: "get"; key: string } | { tool: "write"; op: Op };

const FIELDS: Record<string, string[]> = {
  list: ["status", "type", "query"],
  get: ["key"],
  create: ["title", "description", "status", "priority", "type", "parent", "related_add", "related_remove", "blocked_by_add", "blocked_by_remove"],
  update: ["key", "title", "description", "status", "priority", "type", "branch", "pr", "commit", "parent", "related_add", "related_remove", "blocked_by_add", "blocked_by_remove"],
  log: ["key", "text"],
};

function str(input: Record<string, unknown>, field: string, max: number, required = false): string | undefined {
  const value = input[field];
  if (value === undefined) {
    if (required) throw new Error(`${field} is required.`);
    return undefined;
  }
  if (typeof value !== "string") throw new Error(`${field} must be text.`);
  if (value.length > max) throw new Error(`${field} is limited to ${max.toLocaleString("en")} characters, this one has ${value.length.toLocaleString("en")}.`);
  if (required && !value.trim()) throw new Error(`${field} is required.`);
  return value;
}

function status(input: Record<string, unknown>): Status | undefined {
  if (input.status === undefined) return undefined;
  if (!isStatus(input.status)) throw new Error(`status must be one of ${STATUSES.join(", ")}.`);
  return input.status;
}

function priority(input: Record<string, unknown>): Priority | undefined {
  if (input.priority === undefined) return undefined;
  if (!isPriority(input.priority)) throw new Error(`priority must be one of ${PRIORITIES.join(", ")}.`);
  return input.priority;
}

function type(input: Record<string, unknown>): IssueType | undefined {
  if (input.type === undefined) return undefined;
  if (!isType(input.type)) throw new Error(`type must be one of ${TYPES.join(", ")}.`);
  return input.type;
}

/** A list of issue keys, normalised, up to `MAX_LINKS`. */
function keys(input: Record<string, unknown>, field: string): string[] | undefined {
  const value = input[field];
  if (value === undefined) return undefined;
  if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) throw new Error(`${field} must be a list of issue keys, like ["WIN-3"].`);
  if (value.length > MAX_LINKS) throw new Error(`${field} takes up to ${MAX_LINKS} keys at a time.`);
  return value.map(normalizeKey);
}

/** The link and type fields create and update share. Absent ones stay absent, so an update changes only what it names. */
function relations(input: Record<string, unknown>) {
  return {
    type: type(input),
    parent: str(input, "parent", 20),
    relatedAdd: keys(input, "related_add"),
    relatedRemove: keys(input, "related_remove"),
    blockedByAdd: keys(input, "blocked_by_add"),
    blockedByRemove: keys(input, "blocked_by_remove"),
  };
}

function issueKey(input: Record<string, unknown>): string {
  const key = normalizeKey(input.key);
  if (!key) throw new Error("key is required, like WIN-12.");
  return key;
}

/** Checks a tool's input and says what to do, or throws an Error that tells the agent what to fix. */
export function parseTool(tool: string, input: unknown): ToolCall {
  const fields = FIELDS[tool];
  if (!fields) throw new Error(`Unknown tool ${tool}.`);
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("The input must be an object.");
  const given = input as Record<string, unknown>;
  const stray = Object.keys(given).filter((k) => !fields.includes(k));
  if (stray.length) throw new Error(`Unknown ${stray.length > 1 ? "fields" : "field"}: ${stray.join(", ")}. ${tool} takes ${fields.join(", ")}.`);

  switch (tool) {
    case "list":
      return { tool: "list", status: status(given), type: type(given), query: str(given, "query", 200)?.trim() || undefined };
    case "get":
      return { tool: "get", key: issueKey(given) };
    case "create":
      return { tool: "write", op: { kind: "create", title: str(given, "title", LIMITS.title, true)!, description: str(given, "description", LIMITS.description), status: status(given), priority: priority(given), ...relations(given) } };
    case "update": {
      if (!fields.slice(1).some((f) => given[f] !== undefined)) throw new Error("Pass at least one of title, description, status, priority, type, branch, pr, commit, parent, related_add, related_remove, blocked_by_add or blocked_by_remove.");
      return {
        tool: "write",
        op: {
          kind: "update",
          key: issueKey(given),
          title: str(given, "title", LIMITS.title),
          description: str(given, "description", LIMITS.description),
          status: status(given),
          priority: priority(given),
          branch: str(given, "branch", LIMITS.branch),
          pr: str(given, "pr", LIMITS.pr),
          commit: str(given, "commit", 40),
          ...relations(given),
        },
      };
    }
    default:
      return { tool: "write", op: { kind: "note", key: issueKey(given), text: str(given, "text", LIMITS.note, true)! } };
  }
}
