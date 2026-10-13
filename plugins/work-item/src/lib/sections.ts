// The pure half of the per-project "Work items" section: which iterations are running now, the query for the work
// items assigned to you in them, and the rows. No `wings`, no I/O, so node tests drive it.

export const SECTION_ID = "work-items";
/** The most rows Wings takes for a section. */
export const ROWS_CAP = 50;

/** One node of `classificationNodes/iterations`, as `az devops invoke` prints it. */
export interface IterationNode {
  name?: string;
  path?: string;
  attributes?: { startDate?: string; finishDate?: string };
  children?: IterationNode[];
}

const DAY_MS = 86_400_000;

/**
 * The paths, as work items carry them in `System.IterationPath` (`Project\Sprint 5`), of the iterations running at
 * `now`: dated, started, and not past the end of their last day. A running iteration that has a running child (a
 * release around a sprint) is left out, since items belong to the sprint.
 */
export function currentIterations(root: IterationNode | null | undefined, now: number): string[] {
  const running: IterationNode[] = [];
  const walk = (node: IterationNode) => {
    const start = Date.parse(node.attributes?.startDate ?? "");
    const finish = Date.parse(node.attributes?.finishDate ?? "");
    if (node.path && Number.isFinite(start) && Number.isFinite(finish) && start <= now && now < finish + DAY_MS) running.push(node);
    for (const child of node.children ?? []) walk(child);
  };
  if (root) walk(root);
  return running
    .filter((node) => !running.some((other) => other !== node && other.path!.startsWith(`${node.path}\\`)))
    .map((node) => iterationPath(node.path!));
}

/** `\Project\Iteration\Sprint 5` (a node's path) becomes `Project\Sprint 5` (a work item's). */
export function iterationPath(nodePath: string): string {
  const parts = nodePath.split("\\").filter(Boolean);
  if (parts[1] === "Iteration") parts.splice(1, 1);
  return parts.join("\\");
}

const quote = (s: string) => s.replaceAll("'", "''");

/** WIQL for what's assigned to you in `paths`, newest change first. */
export function assignedQuery(project: string, paths: string[]): string {
  const list = paths.map((p) => `'${quote(p)}'`).join(", ");
  return `SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType] FROM WorkItems WHERE [System.TeamProject] = '${quote(project)}' AND [System.IterationPath] IN (${list}) AND [System.AssignedTo] = @Me AND [System.State] <> 'Removed' ORDER BY [System.ChangedDate] DESC`;
}

/** WIQL for every work item in `paths`, newest change first: the sprint block's "whole sprint", one query per Azure project. */
export function sprintQuery(project: string, paths: string[]): string {
  const list = paths.map((p) => `'${quote(p)}'`).join(", ");
  return `SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType],[System.AssignedTo] FROM WorkItems WHERE [System.TeamProject] = '${quote(project)}' AND [System.IterationPath] IN (${list}) AND [System.State] <> 'Removed' ORDER BY [System.ChangedDate] DESC`;
}

type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";

/** A state's tone from its name: the section has no per-type workflow, and the names differ by process. */
export function stateTone(state: string): Tone {
  if (/^(done|closed|completed|complete)$/i.test(state)) return "success";
  if (/resolved/i.test(state)) return "merged";
  if (/review|qa|test/i.test(state)) return "warning";
  if (/active|progress|doing|committed|develop/i.test(state)) return "info";
  return "neutral";
}

// ---------- which items a repo's section shows ----------

/** Per work item type (lower case), the category of each state (lower case): Proposed, InProgress, Resolved, Completed, Removed. */
export type StateCategories = Record<string, Record<string, string>>;

/** `workItemTypes` as `az devops invoke` prints it, cut by `--query` to the names and categories. */
export function stateCategories(types: { name?: string; states?: { name?: string; category?: string }[] }[] | null | undefined): StateCategories {
  const out: StateCategories = {};
  for (const t of types ?? []) {
    if (!t.name) continue;
    out[t.name.toLowerCase()] = Object.fromEntries((t.states ?? []).filter((s) => s.name && s.category).map((s) => [s.name!.toLowerCase(), s.category!]));
  }
  return out;
}

/** Without the type's categories (the lookup failed, or a custom type): the names processes give their last states. */
const FINISHED_NAMES = /^(done|closed|completed|removed)$/i;

/** A finished item is in a Completed or Removed state category. When the category isn't known, its state's name decides. */
export function isFinished(type: string | undefined, state: string | undefined, categories?: StateCategories): boolean {
  const category = categories?.[(type ?? "").toLowerCase()]?.[(state ?? "").toLowerCase()];
  if (category) return category === "Completed" || category === "Removed";
  return FINISHED_NAMES.test(state ?? "");
}

/** What a work item's relations say about the git repositories it touches: the repository ids in its branch, commit and pull request links (`vstfs:///Git/<kind>/<project id>%2F<repository id>%2F...`), lower case. */
export function repoIdsOf(relations: { rel?: string; url?: string }[] | null | undefined): string[] {
  const ids = new Set<string>();
  for (const r of relations ?? []) {
    const m = r.rel === "ArtifactLink" ? r.url?.match(/^vstfs:\/\/\/Git\/(?:Ref|Commit|PullRequestId)\/[^/]+?%2f([^/]+?)%2f/i) : null;
    if (m) ids.add(m[1].toLowerCase());
  }
  return [...ids];
}

/** `workItems?ids=…&$expand=Relations` as `az devops invoke` prints it, as repository ids by work item id. */
export function repoLinksOf(list: { value?: { id: number; relations?: { rel?: string; url?: string }[] | null }[] } | null | undefined): Record<number, string[]> {
  return Object.fromEntries((list?.value ?? []).map((w) => [w.id, repoIdsOf(w.relations)]));
}

/** Dates and versions in a branch name (2026-10-13, 2026.10, v1.2.3): their numbers aren't work item ids. */
const DATE_OR_VERSION = /\d{4}[-./]\d{1,2}(?:[-./]\d{1,2})?|v?\d+(?:\.\d+)+/gi;

/** Whether a branch name carries the work item's id as a token of its own, at least 3 digits: `feature/1234-login` has 1234, `hotfix/2026-10-13` and `v1.2.3` have none. */
export function branchHasId(branch: string, id: number): boolean {
  if (id < 100) return false;
  return new RegExp(`(?<![A-Za-z0-9])${id}(?![A-Za-z0-9])`).test(branch.replace(DATE_OR_VERSION, " "));
}

export interface RepoRef {
  /** Its Azure repository id, when it could be read. */
  id?: string;
  /** Local and remote branch names. */
  branches: string[];
}

export const NOT_LINKED = "Not linked to a repo";

/**
 * The rows of one repository's section, from the team project's unfinished items. An item shows when it links to the
 * repository (a branch, commit or pull request there) or a branch of it has the item's id, first. An item linked to no
 * repository at all follows, marked "Not linked to a repo", under every repository of the team project. An item linked
 * to other repositories only isn't shown.
 */
export function repoRows(org: string, project: string, rows: QueryRow[], links: Record<number, string[]>, repo: RepoRef): SectionRow[] {
  const mine: QueryRow[] = [];
  const loose: QueryRow[] = [];
  for (const row of rows) {
    const repos = links[row.id] ?? [];
    if ((repo.id && repos.includes(repo.id.toLowerCase())) || repo.branches.some((b) => branchHasId(b, row.id))) mine.push(row);
    else if (repos.length === 0) loose.push(row);
  }
  const out = sectionRows(org, project, [...mine, ...loose]);
  return out.map((r, i) => (i >= mine.length ? { ...r, detail: `${r.detail} · ${NOT_LINKED}` } : r));
}

export interface QueryRow {
  id: number;
  fields: { "System.Title"?: string; "System.State"?: string; "System.WorkItemType"?: string; "System.AssignedTo"?: { displayName?: string } | string };
}

export interface SectionRow {
  id: string;
  label: string;
  detail?: string;
  badge?: { text: string; tone?: Tone };
  open: { view: string; args?: unknown };
}

/** The rows of a section: each opens the item tab for its work item. */
const cut = (s: string, max: number) => (s.length > max ? `${s.slice(0, max - 1)}…` : s);

export function sectionRows(org: string, project: string, rows: QueryRow[]): SectionRow[] {
  return rows.slice(0, ROWS_CAP).map((r) => {
    const state = r.fields["System.State"] ?? "";
    return {
      id: String(r.id),
      label: cut(r.fields["System.Title"] || `#${r.id}`, 120),
      detail: `${r.fields["System.WorkItemType"] ?? "Work item"} #${r.id}`,
      ...(state ? { badge: { text: cut(state, 24), tone: stateTone(state) } } : {}),
      open: { view: "item", args: { id: r.id, org, project } },
    };
  });
}

// ---------- the Sprint block ----------

/** The most work items a block takes. A broadcast is limited to 64 KB, and 100 of these are about 25 KB. */
export const BLOCK_ITEMS_CAP = 100;

/** What the block's state keeps. `mine`: only what's assigned to you; false is the whole sprint. */
export interface SprintBlockState {
  mine: boolean;
}

/** Block state from `wings.blockState()`, which is any JSON the page once saved. Yours by default. */
export function cleanBlockState(raw: unknown): SprintBlockState {
  return { mine: (raw as Partial<SprintBlockState> | null)?.mine !== false };
}

/** A cached read taken at `at` is still good at `now`. */
export const isFresh = (at: number, now: number, ttl: number) => now - at < ttl;

export interface BlockItem {
  id: number;
  title: string;
  type: string;
  state: string;
  assignee?: string;
}

/** The block's page asks main for a project's sprint.  */
export interface SprintRequest {
  type: "sprint-request";
  rid: string;
  path: string;
  mine: boolean;
}

/** `none` is a project that isn't on Azure DevOps; `items` is empty with no running sprint. `total` counts past the cap. */
export interface SprintResponse {
  type: "sprint-response";
  rid: string;
  none?: true;
  org?: string;
  project?: string;
  sprints?: string[];
  items?: BlockItem[];
  total?: number;
  error?: string;
}

export function blockItems(rows: QueryRow[]): BlockItem[] {
  return rows.slice(0, BLOCK_ITEMS_CAP).map((r) => {
    const who = r.fields["System.AssignedTo"];
    const assignee = typeof who === "string" ? who : who?.displayName;
    return {
      id: r.id,
      title: cut(r.fields["System.Title"] || `#${r.id}`, 120),
      type: cut(r.fields["System.WorkItemType"] ?? "Work item", 24),
      state: cut(r.fields["System.State"] || "No state", 24),
      ...(assignee ? { assignee: cut(assignee, 40) } : {}),
    };
  });
}

export interface StateColumn {
  state: string;
  tone: Tone;
  items: BlockItem[];
}

const TONE_ORDER: Tone[] = ["neutral", "info", "warning", "merged", "success", "danger"];

/** One column per state, in the order work moves through them (not started, active, review, resolved, done), each keeping the order the items came in. */
export function groupByState(items: BlockItem[]): StateColumn[] {
  const columns = new Map<string, StateColumn>();
  for (const item of items) {
    const column = columns.get(item.state) ?? { state: item.state, tone: stateTone(item.state), items: [] };
    column.items.push(item);
    columns.set(item.state, column);
  }
  return [...columns.values()].sort((a, b) => TONE_ORDER.indexOf(a.tone) - TONE_ORDER.indexOf(b.tone));
}
