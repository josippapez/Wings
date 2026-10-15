// How issues are ordered and grouped for the board, the list, the short list and the breadcrumb.

import { FLOW, normalizeKey, priorityRank, STATUSES, keyNumber, type Issue, type Status, type Store } from "./domain";

/** The open issues of the short list and the project section: in progress first, then in review, then to do, each oldest key first. */
export function openIssues(issues: Issue[]): Issue[] {
  const order: Record<Status, number> = { in_progress: 0, in_review: 1, todo: 2, done: 3, canceled: 3 };
  return issues.filter((i) => i.status !== "done" && i.status !== "canceled").sort((a, b) => order[a.status] - order[b.status] || keyNumber(a.key) - keyNumber(b.key));
}

/** The board's columns: highest priority first, then to do by key and the rest by latest activity. Canceled only when asked for. */
export function boardColumns(issues: Issue[], showCanceled = false): { status: Status; issues: Issue[] }[] {
  return (showCanceled ? STATUSES : FLOW).map((status) => ({
    status,
    issues: issues.filter((i) => i.status === status).sort((a, b) => priorityRank(a) - priorityRank(b) || (status === "todo" ? keyNumber(a.key) - keyNumber(b.key) : b.updatedAt - a.updatedAt)),
  }));
}

/** A row of the board grouped by parent: an epic or feature and its children, or `parent` null for the issues with none. */
export interface Lane {
  parent: Issue | null;
  columns: { status: Status; issues: Issue[] }[];
  count: number;
}

/**
 * The board in rows, one per parent, like Linear's sub-grouping by parent issue: each issue is a card once, in the row
 * of the epic or feature it sits under. Open parents come first (in progress, in review, to do, then done and canceled),
 * each oldest key first, and the issues with no parent last.
 */
export function boardLanes(issues: Issue[], showCanceled = false): Lane[] {
  const byKey = new Map(issues.map((i) => [i.key, i]));
  const groups = new Map<string, Issue[]>();
  for (const i of issues) {
    if (i.status === "canceled" && !showCanceled) continue;
    const parent = i.parent && byKey.has(i.parent) ? i.parent : "";
    groups.set(parent, [...(groups.get(parent) ?? []), i]);
  }
  const order: Record<Status, number> = { in_progress: 0, in_review: 1, todo: 2, done: 3, canceled: 4 };
  const lane = (parent: Issue | null, members: Issue[]): Lane => ({ parent, columns: boardColumns(members, showCanceled), count: members.length });
  const parents = [...groups.keys()].filter(Boolean).map((k) => byKey.get(k)!);
  parents.sort((a, b) => order[a.status] - order[b.status] || keyNumber(a.key) - keyNumber(b.key));
  const lanes = parents.map((p) => lane(p, groups.get(p.key)!));
  return groups.has("") ? [...lanes, lane(null, groups.get("")!)] : lanes;
}

/** A group of the list by parent: a top-level epic or feature over its children and theirs, each with its depth under it. */
export interface TreeGroup {
  root: Issue;
  rows: { issue: Issue; depth: number }[];
}

/**
 * The list grouped by parent, like Linear's sub-issues: one group per top-level issue that has children, with every
 * issue below it in tree order and indented by `depth` (1 for a child), siblings by status (closest to done first), priority, then
 * key. `loose` is the top-level issues with no children. Canceled ones only when asked for.
 */
export function issueTree(issues: Issue[], showCanceled = false): { groups: TreeGroup[]; loose: Issue[] } {
  const shown = issues.filter((i) => showCanceled || i.status !== "canceled");
  const keys = new Set(shown.map((i) => i.key));
  const order: Record<Status, number> = { in_review: 0, in_progress: 1, todo: 2, done: 3, canceled: 4 };
  const sort = (list: Issue[]) => list.sort((a, b) => order[a.status] - order[b.status] || priorityRank(a) - priorityRank(b) || keyNumber(a.key) - keyNumber(b.key));
  const kids = (key: string) => sort(shown.filter((i) => i.parent === key));
  const below = (key: string, depth: number): TreeGroup["rows"] => kids(key).flatMap((issue) => [{ issue, depth }, ...below(issue.key, depth + 1)]);
  const roots = sort(shown.filter((i) => !i.parent || !keys.has(i.parent)));
  const groups = roots.map((root) => ({ root, rows: below(root.key, 1) })).filter((g) => g.rows.length > 0);
  return { groups, loose: roots.filter((r) => !groups.some((g) => g.root === r)) };
}

/** The issues a tab shows in its breadcrumb at most, oldest dropped first. */
export const TRAIL_LENGTH = 8;
/**
 * The breadcrumb trail for opening `next` from `current`, whose own trail is `trail`: `current` goes on the end. Opening
 * one that's already on the trail goes back to it, so the trail never loops.
 */
export function trailTo(trail: string[], current: string, next: string): string[] {
  const path = [...trail, current];
  const back = path.indexOf(next);
  return (back >= 0 ? path.slice(0, back) : path).slice(-TRAIL_LENGTH);
}

export const findIssue = (store: Store | null, issueKey: string): Issue | null => store?.issues.find((i) => i.key === normalizeKey(issueKey)) ?? null;
