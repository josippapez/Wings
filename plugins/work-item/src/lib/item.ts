// The pure half of the work item tab: reading what `az boards work-item show --expand relations` and the work item's
// updates return. No `wings`, no I/O, so node tests drive it.

import type { QueryRow } from "./sections.ts";
import { updateTime, type WorkItemUpdate } from "./urls.ts";

/** What the "item" view is opened with. `project` is optional: the page learns it from the work item. */
export interface ItemArgs {
  id: number;
  org: string;
  project?: string;
}

/** An `args` value from `wings.viewArgs()` as an `ItemArgs`, or null when it isn't one. */
export function itemArgs(raw: unknown): ItemArgs | null {
  const a = raw as Partial<ItemArgs> | null;
  if (!a || !Number.isInteger(a.id) || (a.id as number) <= 0 || typeof a.org !== "string" || !a.org.startsWith("https://")) return null;
  return { id: a.id as number, org: a.org, ...(typeof a.project === "string" && a.project ? { project: a.project } : {}) };
}

export interface Relation {
  rel?: string;
  url?: string;
  attributes?: { name?: string };
}

export interface PrLink {
  id: number;
  /** The repository's id: web links accept it where a name goes. */
  repoId: string;
}

/** The pull requests linked to a work item: `ArtifactLink`s named "Pull Request", `vstfs:///Git/PullRequestId/<project>%2F<repo>%2F<id>`. */
export function pullRequestLinks(relations: Relation[] | null | undefined): PrLink[] {
  const out: PrLink[] = [];
  for (const r of relations ?? []) {
    if (r.rel !== "ArtifactLink" || r.attributes?.name !== "Pull Request") continue;
    const m = r.url?.match(/^vstfs:\/\/\/Git\/PullRequestId\/[^/]+?%2f([^/]+?)%2f(\d+)$/i);
    if (m && !out.some((p) => p.id === Number(m[2]))) out.push({ repoId: m[1], id: Number(m[2]) });
  }
  return out;
}

export interface Comment {
  id: number;
  author: string;
  at: string;
  /** HTML, as Azure DevOps keeps it. Never put it in a page as it is: see `Html`. */
  html: string;
}

/** The comments of a work item: each revision that set `System.History`, oldest first. */
export function commentsOf(updates: WorkItemUpdate[]): Comment[] {
  const out: Comment[] = [];
  for (const u of updates) {
    const history = u.fields?.["System.History"]?.newValue;
    if (typeof history !== "string" || !history.trim()) continue;
    out.push({ id: u.id, author: u.revisedBy?.displayName ?? "Someone", at: updateTime(u), html: history });
  }
  return out.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

// ---------- fields ----------

/** The fields of a work item the tab reads: everything is optional, since a process leaves out what it doesn't use. */
export interface Fields {
  "System.Title": string;
  "System.TeamProject": string;
  "System.WorkItemType": string;
  "System.State": string;
  "System.AreaPath"?: string;
  "System.IterationPath"?: string;
  "System.Parent"?: number;
  "System.AssignedTo"?: { displayName?: string };
  "System.CreatedBy"?: { displayName?: string };
  "System.CreatedDate"?: string;
  "System.ChangedBy"?: { displayName?: string };
  "System.ChangedDate"?: string;
  "System.Description"?: string;
  "System.Tags"?: string;
  "Microsoft.VSTS.Common.AcceptanceCriteria"?: string;
  "Microsoft.VSTS.TCM.ReproSteps"?: string;
  "Microsoft.VSTS.Common.Priority"?: number;
  "Microsoft.VSTS.Common.Severity"?: string;
  "Microsoft.VSTS.Scheduling.StoryPoints"?: number;
  "Microsoft.VSTS.Scheduling.Effort"?: number;
  "Microsoft.VSTS.Scheduling.Size"?: number;
  "Microsoft.VSTS.Scheduling.RemainingWork"?: number;
  "Microsoft.VSTS.Scheduling.CompletedWork"?: number;
}

/** The last part of an area or iteration path: `Proj\\Team\\Sprint 5` is `Sprint 5`. */
export const lastSegment = (path: string | undefined) => (path ? (path.split("\\").filter(Boolean).at(-1) ?? "") : "");

/** `a; b; c` as `["a", "b", "c"]`. */
export const tagsOf = (tags: string | undefined) => (tags ?? "").split(";").map((t) => t.trim()).filter(Boolean);

export interface Summary {
  priority?: number;
  severity?: string;
  /** Story points, effort or size, whichever the process uses. */
  estimate?: { label: string; value: number };
  remaining?: number;
  completed?: number;
}

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);

/** The numbers worth a chip: priority, severity, the estimate and the work left. */
export function summaryOf(f: Partial<Fields>): Summary {
  const estimate = ([["Story points", f["Microsoft.VSTS.Scheduling.StoryPoints"]], ["Effort", f["Microsoft.VSTS.Scheduling.Effort"]], ["Size", f["Microsoft.VSTS.Scheduling.Size"]]] as const).find(([, v]) => num(v) !== undefined);
  return {
    ...(num(f["Microsoft.VSTS.Common.Priority"]) !== undefined ? { priority: f["Microsoft.VSTS.Common.Priority"] } : {}),
    ...(f["Microsoft.VSTS.Common.Severity"] ? { severity: f["Microsoft.VSTS.Common.Severity"].replace(/^\d+\s*-\s*/, "") } : {}),
    ...(estimate ? { estimate: { label: estimate[0], value: estimate[1] as number } } : {}),
    ...(num(f["Microsoft.VSTS.Scheduling.RemainingWork"]) !== undefined ? { remaining: f["Microsoft.VSTS.Scheduling.RemainingWork"] } : {}),
    ...(num(f["Microsoft.VSTS.Scheduling.CompletedWork"]) !== undefined ? { completed: f["Microsoft.VSTS.Scheduling.CompletedWork"] } : {}),
  };
}

// ---------- links ----------

/** The work items a work item links to, by id. Parent, child, related, predecessor and successor are Azure's own link types. */
export interface LinkIds {
  parent: number | null;
  children: number[];
  related: number[];
  predecessors: number[];
  successors: number[];
}

const LINK_GROUP: Record<string, keyof Omit<LinkIds, "parent">> = {
  "System.LinkTypes.Hierarchy-Forward": "children",
  "System.LinkTypes.Related": "related",
  "System.LinkTypes.Dependency-Reverse": "predecessors",
  "System.LinkTypes.Dependency-Forward": "successors",
};

/** The most linked work items the tab looks up in one query. */
export const LINKS_CAP = 40;

/** The work item links among `relations` (a link's url ends in `/workItems/<id>`), grouped; the parent is the `System.Parent` field when the relations don't say. */
export function linkIds(relations: Relation[] | null | undefined, parentField?: number): LinkIds {
  const out: LinkIds = { parent: parentField ?? null, children: [], related: [], predecessors: [], successors: [] };
  for (const r of relations ?? []) {
    const id = Number(r.url?.match(/\/workItems\/(\d+)$/i)?.[1]);
    if (!Number.isInteger(id) || id <= 0) continue;
    if (r.rel === "System.LinkTypes.Hierarchy-Reverse") out.parent = id;
    else if (r.rel && LINK_GROUP[r.rel] && !out[LINK_GROUP[r.rel]].includes(id)) out[LINK_GROUP[r.rel]].push(id);
  }
  return out;
}

/** Every id in `links`, once, at most `LINKS_CAP`: parent first, then children, related, predecessors, successors. */
export function allLinkIds(links: LinkIds): number[] {
  return [...new Set([...(links.parent ? [links.parent] : []), ...links.children, ...links.related, ...links.predecessors, ...links.successors])].slice(0, LINKS_CAP);
}

/** WIQL for the title, state and type of `ids`: one query for all the links, instead of one `show` each. Only integers go in. */
export function idsQuery(ids: number[]): string {
  return `SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType],[System.AssignedTo] FROM WorkItems WHERE [System.Id] IN (${ids.filter((n) => Number.isInteger(n) && n > 0).join(",")})`;
}

export interface LinkedItem {
  id: number;
  title: string;
  state: string;
  type: string;
  assignee?: string;
}

/** What `idsQuery` returns, by id. */
export function linkedItems(rows: QueryRow[]): Record<number, LinkedItem> {
  const out: Record<number, LinkedItem> = {};
  for (const r of rows) {
    const who = r.fields["System.AssignedTo"];
    const assignee = typeof who === "string" ? who : who?.displayName;
    out[r.id] = { id: r.id, title: r.fields["System.Title"] ?? `#${r.id}`, state: r.fields["System.State"] ?? "", type: r.fields["System.WorkItemType"] ?? "Work item", ...(assignee ? { assignee } : {}) };
  }
  return out;
}

/** A linked pull request's status as Azure names it, as words and a tone for its badge. */
export function prStatus(status: string | undefined): { label: string; tone?: "info" | "success" } {
  if (status === "active") return { label: "Active", tone: "info" };
  if (status === "completed") return { label: "Completed", tone: "success" };
  if (status === "abandoned") return { label: "Abandoned" };
  return { label: status ?? "" };
}

// ---------- history ----------

/** One field of a revision that changed. A missing `from` was empty before, a missing `to` was cleared. */
export interface Change {
  label: string;
  from?: string;
  to?: string;
}

export interface Revision {
  id: number;
  by: string;
  at: string;
  /** The first revision: what the work item started with. */
  created: boolean;
  changes: Change[];
}

const TRACKED: [string, string][] = [
  ["System.State", "State"],
  ["System.AssignedTo", "Assigned to"],
  ["System.IterationPath", "Sprint"],
  ["System.AreaPath", "Area"],
  ["System.Title", "Title"],
  ["Microsoft.VSTS.Common.Priority", "Priority"],
  ["Microsoft.VSTS.Common.Severity", "Severity"],
  ["Microsoft.VSTS.Scheduling.StoryPoints", "Story points"],
  ["Microsoft.VSTS.Scheduling.Effort", "Effort"],
  ["Microsoft.VSTS.Scheduling.RemainingWork", "Remaining work"],
  ["System.Tags", "Tags"],
];

/** A changed value as text: a person by name, a path by its last part, nothing as undefined. */
function valueText(field: string, v: unknown): string | undefined {
  if (v === null || v === undefined || v === "") return undefined;
  if (typeof v === "object") return (v as { displayName?: string }).displayName || undefined;
  const text = String(v);
  return field === "System.IterationPath" || field === "System.AreaPath" ? lastSegment(text) : text;
}

/** The revisions that changed something we show, oldest first. A revision that only added a comment, or touched fields we don't track, is left out. */
export function revisionsOf(updates: WorkItemUpdate[]): Revision[] {
  const sorted = [...updates].sort((a, b) => Date.parse(updateTime(a)) - Date.parse(updateTime(b)) || a.id - b.id);
  const out: Revision[] = [];
  sorted.forEach((u, index) => {
    const changes: Change[] = [];
    for (const [field, label] of TRACKED) {
      const c = u.fields?.[field];
      if (!c) continue;
      const from = valueText(field, c.oldValue);
      const to = valueText(field, c.newValue);
      if (from === to) continue;
      changes.push({ label, ...(from !== undefined ? { from } : {}), ...(to !== undefined ? { to } : {}) });
    }
    const created = index === 0;
    if (changes.length || created) out.push({ id: u.id, by: u.revisedBy?.displayName ?? "Someone", at: updateTime(u), created, changes });
  });
  return out;
}

// ---------- discussion ----------

export type Activity =
  | { kind: "comment"; key: string; author: string; at: string; html: string }
  | { kind: "change"; key: string; author: string; at: string; created: boolean; changes: Change[] };

/** The fields whose changes read as part of the conversation; the History tab has all of them. */
const CONVERSATION = new Set(["State", "Assigned to", "Sprint"]);

/** Comments and the changes that matter to a conversation (state, assignee, sprint, and its creation), newest first. In time order a comment comes before a change made in the same revision, so newest first puts it after. */
export function discussionOf(comments: Comment[], revisions: Revision[]): Activity[] {
  const out: Activity[] = comments.map((c) => ({ kind: "comment" as const, key: `c${c.id}`, author: c.author, at: c.at, html: c.html }));
  for (const r of revisions) {
    const changes = r.created ? [] : r.changes.filter((c) => CONVERSATION.has(c.label));
    if (changes.length || r.created) out.push({ kind: "change", key: `r${r.id}`, author: r.by, at: r.at, created: r.created, changes });
  }
  return out.sort((a, b) => Date.parse(a.at) - Date.parse(b.at) || (a.kind === b.kind ? 0 : a.kind === "comment" ? -1 : 1)).reverse();
}
