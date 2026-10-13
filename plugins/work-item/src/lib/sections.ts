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

type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "merged";

/** A state's tone from its name: the section has no per-type workflow, and the names differ by process. */
export function stateTone(state: string): Tone {
  if (/^(done|closed|completed|complete)$/i.test(state)) return "success";
  if (/resolved/i.test(state)) return "merged";
  if (/review|qa|test/i.test(state)) return "warning";
  if (/active|progress|doing|committed|develop/i.test(state)) return "info";
  return "neutral";
}

export interface QueryRow {
  id: number;
  fields: { "System.Title"?: string; "System.State"?: string; "System.WorkItemType"?: string };
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
