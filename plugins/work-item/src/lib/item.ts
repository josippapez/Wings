// The pure half of the work item tab: reading what `az boards work-item show --expand relations` and the work item's
// updates return. No `wings`, no I/O, so node tests drive it.

import { updateTime, type WorkItemUpdate } from "./updates.ts";

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
