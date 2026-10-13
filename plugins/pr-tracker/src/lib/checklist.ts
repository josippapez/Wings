// The Checks view's data: every check or policy of a PR as one list, however its provider words it, and the groups it
// shows in. No `wings`, no I/O, so node tests drive it.
import type { AzurePolicy, AzurePr, CheckItem, CheckState, Failing } from "./types.ts";

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
export function groupOf(c: CheckItem): GroupId {
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
function real(iso: string | null | undefined): string | null {
  return iso && Date.parse(iso) > Date.parse("2000-01-01") ? iso : null;
}

/** The note a comment requirement shows once the threads are known. */
export function withUnresolved(items: CheckItem[], unresolved: number | null): CheckItem[] {
  return items.map((c) => {
    if (c.needs !== "unresolved" || unresolved === null) return c;
    const note = unresolved ? `${unresolved} comment ${unresolved === 1 ? "thread" : "threads"} unresolved` : "All comment threads resolved";
    return { ...c, note };
  });
}

// ---------- GitHub ----------

/** A CheckRun or a StatusContext in `gh pr view --json statusCheckRollup`. */
export interface GhCheck {
  __typename?: string;
  state?: string;
  status?: string;
  conclusion?: string;
  detailsUrl?: string;
  targetUrl?: string;
  name?: string;
  context?: string;
  description?: string;
  workflowName?: string;
  startedAt?: string;
  completedAt?: string;
}

const GH_FAILED = ["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"];
const GH_NOTE: Record<string, string> = { TIMED_OUT: "Timed out", CANCELLED: "Cancelled", ACTION_REQUIRED: "Action required", STARTUP_FAILURE: "Failed to start" };

export function ghState(c: GhCheck): CheckState {
  if (c.__typename === "StatusContext") {
    if (c.state === "SUCCESS") return "passed";
    if (c.state === "EXPECTED") return "queued";
    return GH_FAILED.includes(c.state!) ? "failed" : "running";
  }
  if (c.status === "COMPLETED") {
    if (c.conclusion === "SUCCESS" || c.conclusion === "NEUTRAL") return "passed";
    if (c.conclusion === "SKIPPED") return "skipped";
    return GH_FAILED.includes(c.conclusion!) ? "failed" : "running";
  }
  return c.status === "IN_PROGRESS" ? "running" : "queued";
}

/** `host` is the PR's host: only an Actions job there has a log to fetch. */
/** `index` is the check's place in the rollup: two contexts can share a name or a link, and each needs its own key. */
export function ghItem(c: GhCheck, host: string, index = 0): CheckItem {
  const url = c.detailsUrl ?? c.targetUrl ?? null;
  const name = c.name ?? c.context ?? "Check";
  const state = ghState(c);
  const job = url?.startsWith(`https://${host}/`) ? url.match(/\/actions\/runs\/\d+\/job\/(\d+)/)?.[1] : undefined;
  const note = c.description || (c.conclusion && GH_NOTE[c.conclusion]) || null;
  const failing: Failing = { id: (url ?? c.name ?? c.context)!, name: c.name ?? c.context, url, job, summary: c.description || null };
  return {
    id: `${name}|${url ?? ""}|${index}`,
    name,
    state,
    // `gh pr view` doesn't say which checks branch protection requires.
    required: null,
    kind: c.workflowName || (c.__typename === "StatusContext" ? "Status check" : "Check run"),
    at: real(c.completedAt) ?? real(c.startedAt),
    seconds: between(c.startedAt, c.completedAt),
    url,
    note,
    failing: state === "failed" ? failing : undefined,
  };
}

/** Every failed check's `Failing`, which the badge and the MCP tool list. */
export const failingOf = (items: CheckItem[]) => items.flatMap((c) => (c.state === "failed" && c.failing ? [c.failing] : []));

// ---------- GitLab ----------

export interface GlJobItem {
  id: number;
  name: string;
  status: string;
  stage?: string;
  allow_failure?: boolean;
  web_url: string;
  started_at?: string | null;
  finished_at?: string | null;
  duration?: number | null;
}

const GL_QUEUED = ["created", "pending", "scheduled", "waiting_for_resource", "waiting_for_callback"];

export function glItem(j: GlJobItem, projectId: number): CheckItem {
  let state: CheckState;
  let note: string | null = null;
  if (j.status === "success") state = "passed";
  else if (j.status === "skipped") [state, note] = ["skipped", "Skipped"];
  else if (j.status === "manual") [state, note] = ["skipped", "Manual job: start it in GitLab"];
  else if (j.status === "failed" && j.allow_failure) [state, note] = ["passed", "Failed, but allowed to fail"];
  else if (j.status === "failed") state = "failed";
  else if (j.status === "canceled" || j.status === "canceling") [state, note] = ["failed", "Canceled"];
  else state = GL_QUEUED.includes(j.status) ? "queued" : "running";
  return {
    id: String(j.id),
    name: j.name,
    state,
    required: j.allow_failure ? false : true,
    kind: j.stage ? `Job in ${j.stage}` : "Job",
    at: real(j.finished_at) ?? real(j.started_at),
    seconds: j.duration ?? null,
    url: j.web_url,
    note,
    failing: state === "failed" ? { id: j.id, name: j.name, job: j.id, project: projectId, url: j.web_url, summary: j.status === "canceled" ? "Canceled" : null } : undefined,
  };
}

// ---------- Bitbucket Cloud and Gitea ----------

export interface BbStatusItem {
  key: string;
  type?: string;
  state: string;
  name?: string;
  description?: string;
  url?: string;
  created_on?: string;
  updated_on?: string;
}

export function bbItem(s: BbStatusItem): CheckItem {
  const state: CheckState = s.state === "SUCCESSFUL" ? "passed" : s.state === "FAILED" || s.state === "STOPPED" ? "failed" : "running";
  return {
    id: s.key,
    name: s.name ?? s.key,
    state,
    required: null,
    kind: s.type === "build" ? "Build" : "Status check",
    at: real(s.updated_on) ?? real(s.created_on),
    seconds: null,
    url: s.url ?? null,
    note: s.description || (s.state === "STOPPED" ? "Stopped" : null),
  };
}

export interface GtStatusItem {
  status: string;
  context?: string;
  description?: string;
  target_url?: string;
  created_at?: string;
  updated_at?: string;
}

export function gtItem(s: GtStatusItem): CheckItem {
  const state: CheckState = s.status === "success" || s.status === "warning" ? "passed" : s.status === "skipped" ? "skipped" : s.status === "pending" ? "running" : "failed";
  return {
    id: s.context ?? s.target_url ?? "check",
    name: s.context ?? "Check",
    state,
    required: null,
    kind: "Status check",
    at: real(s.updated_at) ?? real(s.created_at),
    seconds: null,
    url: s.target_url ?? null,
    note: s.description || (s.status === "warning" ? "Warning" : null),
  };
}

// ---------- Azure DevOps ----------

/** Azure's policy types, by the name its policy list gives them. Build and the two below are told apart by what the record holds. */
const AZURE_KINDS: [RegExp, string, CheckItem["needs"]?][] = [
  [/comment/i, "Comment requirements", "unresolved"],
  [/work item/i, "Work item linking"],
  [/required reviewers|automatically included/i, "Required reviewers"],
  [/minimum|number of reviewers/i, "Minimum reviewers"],
  [/merge strategy/i, "Merge strategy"],
  [/build/i, "Build"],
  [/status/i, "Status check"],
];

const azureState = (status: string): CheckState =>
  status === "approved" ? "passed" : status === "rejected" || status === "broken" ? "failed" : status === "running" ? "running" : status === "queued" ? "queued" : "skipped";

/** A pull request's policies as checks. `org` is the organization's URL. A disabled policy isn't one. */
export function azureItems(policies: AzurePolicy[], pr: AzurePr, org: string, project: string): CheckItem[] {
  const reviewers = pr.reviewers ?? [];
  const required = reviewers.filter((r) => r.isRequired);
  const waiting = required.filter((r) => !(r.vote > 0)).map((r) => r.displayName ?? "unknown");
  const approvals = reviewers.filter((r) => r.vote > 0).length;
  return policies
    .filter((p) => p.configuration?.isEnabled !== false)
    .map((p, i): CheckItem => {
      const typeName = p.configuration?.type?.displayName ?? "";
      const settings = p.configuration?.settings ?? {};
      const [, kind, needs] = AZURE_KINDS.find(([re]) => re.test(typeName)) ?? [null, p.context?.buildId ? "Build" : typeName || "Policy"];
      const state = azureState(p.status);
      const build = p.context?.buildId ?? null;
      const name = String(settings.displayName ?? p.context?.buildDefinitionName ?? settings.statusName ?? (typeName || "Policy"));
      let note: string | null = null;
      if (state === "skipped") note = "Doesn't apply to this pull request";
      else if (kind === "Required reviewers" && required.length) {
        note = `${required.length - waiting.length} of ${required.length} required reviewers approved${waiting.length ? `. Waiting for ${waiting.join(", ")}` : ""}`;
      } else if (kind === "Minimum reviewers") {
        const need = Number(settings.minimumApproverCount);
        if (need > 0) note = `${Math.min(approvals, need)} of ${need} approvals`;
      } else if (kind === "Work item linking") note = state === "passed" ? "A work item is linked" : "Link a work item to this pull request";
      else if (kind === "Comment requirements" && state !== "passed") note = "Comment threads are unresolved";
      else if (kind === "Merge strategy" && state !== "passed") note = "Pick the merge type when completing";
      else if (state === "failed" && !build) note = p.status === "broken" ? "The policy hit an error" : "Not met";
      else if (state === "failed") note = p.status === "broken" ? "The build policy hit an error" : "The build failed";
      else if (state === "queued" && build) note = "Waiting for the build to start";
      return {
        id: `${p.configuration?.type?.id ?? "policy"}:${build ?? name}:${i}`,
        name,
        state: kind === "Merge strategy" && state === "failed" ? "queued" : state,
        required: typeof p.configuration?.isBlocking === "boolean" ? p.configuration.isBlocking : null,
        kind,
        at: real(p.completedDate) ?? real(p.startedDate),
        seconds: between(p.startedDate, p.completedDate),
        url: build ? `${org}/${encodeURIComponent(project)}/_build/results?buildId=${build}` : null,
        note,
        needs,
        failing: state === "failed" && build ? { id: build, name, build } : undefined,
      };
    });
}

/** Whether a failed check has a log to fetch: a GitHub Actions job, a GitLab job or an Azure build. */
export const hasLog = (f: Failing | undefined) => Boolean(f && (f.job || f.build));

const SHOWN_LINES = 40;

/** The end of a log for the page: at most `SHOWN_LINES` lines, and how many came before them. */
export function logTail(log: string, max = SHOWN_LINES): { text: string; hidden: number } {
  const lines = log.split("\n");
  return { text: lines.slice(-max).join("\n"), hidden: Math.max(0, lines.length - max) };
}
