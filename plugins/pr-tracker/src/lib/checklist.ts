// The Checks view's data: every check or policy of a PR as one list, however its provider words it, and the groups it
// shows in. Each provider's own mapper builds the `CheckItem`s. No `wings`, no I/O, so node tests drive it.
import { plural } from "wings-plugin-sdk/format";
import type { CheckItem, CheckState, Failing } from "./types";

// ---------- grouping ----------

export type GroupId = "failed" | "active" | "passed" | "other";

export interface Group {
  id: GroupId;
  label: string;
  items: CheckItem[];
}

const LABELS: Record<GroupId, string> = { failed: "Failed", active: "Running and queued", passed: "Passed", other: "Optional and not run" };
const ORDER: GroupId[] = ["failed", "active", "passed", "other"];

/** Where a check shows: what failed first, then what is still going, then what passed; optional passes and checks that didn't run last. */
function groupOf(c: CheckItem): GroupId {
  if (c.state === "failed") return "failed";
  if (c.state === "running" || c.state === "queued") return "active";
  if (c.state === "skipped" || c.required === false) return "other";
  return "passed";
}

const RANK: Record<CheckState, number> = { failed: 0, running: 1, queued: 2, passed: 3, skipped: 4 };

/** The non-empty groups in display order. Inside a group a required check comes first, a running one before a queued one, then by name. */
export function groupChecks(items: CheckItem[]): Group[] {
  const sorted = [...items].sort((a, b) => Number(b.required === true) - Number(a.required === true) || RANK[a.state] - RANK[b.state] || a.name.localeCompare(b.name));
  return ORDER.map((id) => ({ id, label: LABELS[id], items: sorted.filter((c) => groupOf(c) === id) })).filter((g) => g.items.length > 0);
}

/** "45s", "2m 14s", "1h 05m". */
export function durationText(seconds: number | null): string | null {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return null;
  const s = Math.round(seconds);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/** The seconds between two ISO times, or null when either is missing, a provider's "never" date or out of order. */
export function between(start: string | null | undefined, end: string | null | undefined): number | null {
  const [a, b] = [real(start), real(end)];
  return a && b && b >= a ? (Date.parse(b) - Date.parse(a)) / 1000 : null;
}

/** GitHub and Azure DevOps send year 1 or 0001 for a time that hasn't happened. */
export function real(iso: string | null | undefined): string | null {
  return iso && Date.parse(iso) > Date.parse("2000-01-01") ? iso : null;
}

/** The note a comment requirement shows once the threads are known. */
export function withUnresolved(items: CheckItem[], unresolved: number | null): CheckItem[] {
  return items.map((c) => {
    if (c.needs !== "unresolved" || unresolved === null) return c;
    const note = unresolved ? `${plural(unresolved, "comment thread")} unresolved` : "All comment threads resolved";
    return { ...c, note };
  });
}

/** Every failed check's `Failing`, which the badge and the MCP tool list. */
export const failingOf = (items: CheckItem[]) => items.flatMap((c) => (c.state === "failed" && c.failing ? [c.failing] : []));

/** Whether a failed check has a log to fetch: a GitHub Actions job, a GitLab job or an Azure build. */
export const hasLog = (f: Failing | undefined) => Boolean(f && (f.job || f.build));

const SHOWN_LINES = 40;

/** The end of a log for the page: at most `SHOWN_LINES` lines, and how many came before them. */
export function logTail(log: string, max = SHOWN_LINES): { text: string; hidden: number } {
  const lines = log.split("\n");
  return { text: lines.slice(-max).join("\n"), hidden: Math.max(0, lines.length - max) };
}
