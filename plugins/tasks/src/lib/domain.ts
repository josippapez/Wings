// What an issue is: the types, the limits that keep a project inside the plugin's storage, the labels, and the small rules about keys
// and descriptions that every other module shares. Nothing here touches Wings.

/**
 * Why these numbers: each issue is its own storage entry, and Wings holds 1 MB per entry and 50 MB for the whole plugin,
 * shared by every project. An issue with a short description and ten one-line log entries is about 2 KB of JSON (see
 * `estimateBytes` and the test that measures it), so 300 of them are about 600 KB, and a busy issue with a long spec and
 * notes is still tens of KB. `ISSUE_BUDGET` stops one issue before the 1 MB wall, with a message instead of storage's own
 * refusal: an issue with 50 notes of the full 20,000 characters is about 1 MB.
 * A broadcast is limited to 64,000 characters, and none carries issue text from main to a page: it carries ids.
 * `key`, `commit` and `query` only bound what a tool accepts: a key like WEB-12 is 13 characters at most, a commit hash 40.
 */
export const LIMITS = { issues: 300, title: 200, description: 20_000, note: 20_000, branch: 200, pr: 500, log: 300, key: 20, commit: 40, query: 200, files: 100, filePath: 300, fileWhat: 300 } as const;
/** The most one issue's entry may take, in bytes of JSON. */
export const ISSUE_BUDGET = 1_000_000;
export const FORMAT_VERSION = 3;
/** Links one issue keeps to others, of each kind. */
export const MAX_LINKS = 20;

export type Status = "todo" | "in_progress" | "in_review" | "done" | "canceled";
export const STATUSES: Status[] = ["todo", "in_progress", "in_review", "done", "canceled"];
/** The columns of the board, in the order an issue moves through them. Canceled sits behind a toggle. */
export const FLOW: Status[] = ["todo", "in_progress", "in_review", "done"];
export const STATUS_LABEL: Record<Status, string> = { todo: "To do", in_progress: "In progress", in_review: "In review", done: "Done", canceled: "Canceled" };

export type IssueType = "task" | "bug" | "feature" | "epic";
export const TYPES: IssueType[] = ["task", "bug", "feature", "epic"];
export const TYPE_LABEL: Record<IssueType, string> = { task: "Task", bug: "Bug", feature: "Feature", epic: "Epic" };
/** Linear's priorities. "none" is an issue without one: it isn't stored. */
export type Priority = "urgent" | "high" | "medium" | "low" | "none";
export const PRIORITIES: Priority[] = ["urgent", "high", "medium", "low", "none"];
export const PRIORITY_LABEL: Record<Priority, string> = { urgent: "Urgent", high: "High", medium: "Medium", low: "Low", none: "No priority" };
/** Sorts urgent first and no priority last. */
export const priorityRank = (issue: { priority?: Priority }): number => PRIORITIES.indexOf(issue.priority ?? "none");
/** Only these can be somebody's parent. */
export const canParent = (type: IssueType): boolean => type === "epic" || type === "feature";

/**
 * Who wrote a log entry: the user in the UI, "git" for what the plugin found in the repository by itself (a commit or
 * branch with the issue's key), or the agent session a tool call came from. "git" is its own kind rather than
 * "you" so the log never claims the user did something they didn't do.
 */
export type Actor = "you" | "git" | { id: string; name?: string; kind?: AgentKind; via?: "journal" };
/** Which agent CLI a session belongs to. Absent on sessions recorded before there was more than Claude Code. */
export type AgentKind = WingsAgentKind;
export type LogKind = "created" | "status" | "note" | "link" | "type" | "priority";
export interface LogEntry {
  at: number;
  by: Actor;
  kind: LogKind;
  text: string;
}
export interface SessionRef {
  id: string;
  name?: string;
  kind?: AgentKind;
  /** When a tool call from this session last touched the issue. */
  seen: number;
}
/** One file an agent changed on this issue, as its change journal described it. Another tool's output, so the texts are cut, not refused. */
export interface FileChange {
  path: string;
  what: string;
  /** When the newest turn that changed it reported it. */
  at: number;
  /** The agent session of that turn. */
  session?: { id: string; name?: string; kind?: AgentKind };
}
export interface Issue {
  /** Like `WIN-12`. Never reused. */
  key: string;
  title: string;
  /** Markdown: goal, scope, acceptance criteria. */
  description?: string;
  status: Status;
  type: IssueType;
  /** Absent for no priority. */
  priority?: Exclude<Priority, "none">;
  /** The epic or feature this belongs to. Same project, never itself or one of its own children. */
  parent?: string;
  /** Symmetric: if A lists B, B lists A. */
  related?: string[];
  /** The issues that have to finish first. Their `blocks` is worked out from this, not stored. */
  blockedBy?: string[];
  links?: { branch?: string; pr?: string; commits?: string[] };
  /** One row per path, newest first, up to `LIMITS.files`. Absent on issues no journal reported on; the log stays for people and decisions. */
  files?: FileChange[];
  /** Append-only and deduped by id. */
  sessions: SessionRef[];
  /** Append-only. Nothing edits or removes an entry. */
  log: LogEntry[];
  createdAt: number;
  updatedAt: number;
}
/** What one project's storage entry holds. `next` is the number the next issue gets, and only ever grows. */
export interface Store {
  v: typeof FORMAT_VERSION;
  prefix: string;
  next: number;
  issues: Issue[];
}

/** What a page or a tool asks main to do. For `update`, `branch` and `pr` "" clear the link. */
export interface Relations {
  type?: IssueType;
  /** A key, or "" to clear. */
  parent?: string;
  relatedAdd?: string[];
  relatedRemove?: string[];
  blockedByAdd?: string[];
  blockedByRemove?: string[];
}
export type Op =
  | ({ kind: "create"; title: string; description?: string; status?: Status; priority?: Priority; branch?: string } & Relations)
  | ({ kind: "update"; key: string; title?: string; description?: string; status?: Status; priority?: Priority; branch?: string; pr?: string; commit?: string } & Relations)
  | { kind: "note"; key: string; text: string }
  /** Records the acting session on the issue without a log entry. Nothing changes when it is already there. */
  | { kind: "touch"; key: string }
  /** What an agent's journal says a turn changed: one row per path, the newest description wins. Writes no log entry. */
  | { kind: "files"; key: string; files: { path: string; what: string }[] }
  /** With `children`, its child issues, theirs and so on go too. Otherwise they stay and lose their parent. */
  | { kind: "remove"; key: string; children?: boolean };

/** What an empty description starts as when it is first edited on the issue page. */
export const DESCRIPTION_TEMPLATE = "## Goal\n\n## Scope\n\n## Done when\n- [ ] \n\n## Validation\n";

/**
 * The description without its empty sections: a `## heading` whose body is blank or only empty checklist items (`- [ ]`)
 * is dropped, and text before the first heading stays. The untouched template comes out as "".
 */
export function visibleDescription(description: string | undefined): string {
  const parts = (description ?? "").split(/^(?=## )/m);
  const body = (part: string) => part.replace(/^## .*$/m, "").replace(/^\s*[-*]\s*\[ \]\s*$/gm, "").trim();
  return parts.filter((part) => (part.startsWith("## ") ? body(part) : part.trim())).join("").trim();
}
/** Whether a description says anything beyond the template's headings and empty checklist items. */
export const hasContent = (description: string | undefined): boolean => visibleDescription(description) !== "";

export const isType = (v: unknown): v is IssueType => typeof v === "string" && (TYPES as string[]).includes(v);
export const isPriority = (v: unknown): v is Priority => typeof v === "string" && (PRIORITIES as string[]).includes(v);
export const isStatus = (v: unknown): v is Status => typeof v === "string" && (STATUSES as string[]).includes(v);

// ---------- keys ----------

/** Up to three letters of the project's name, uppercase, or ISS when it has none. "my-app" is MYA. */
export function prefixFor(name: string): string {
  return (name.match(/[A-Za-z]/g) ?? []).slice(0, 3).join("").toUpperCase() || "ISS";
}
export const keyNumber = (key: string): number => Number(key.slice(key.lastIndexOf("-") + 1));
export const KEY_PATTERN = /^[A-Z]{1,3}-\d{1,9}$/;
/** What a person or an agent typed, as a key: trimmed and uppercased. */
export const normalizeKey = (value: unknown): string => (typeof value === "string" ? value.trim().toUpperCase() : "");
