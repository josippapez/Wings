import { nestWorktrees } from "./worktree-agent.ts";

/** How the sidebar orders projects. Manual is the order you dragged them into. */
export type ProjectSort = "manual" | "added" | "worked" | "name";

export const projectSorts: { value: ProjectSort; label: string }[] = [
  { value: "manual", label: "Manual" },
  { value: "added", label: "Added" },
  { value: "worked", label: "Last worked on" },
  { value: "name", label: "Name" },
];

export const parseProjectSort = (saved: unknown): ProjectSort => (projectSorts.some((s) => s.value === saved) ? (saved as ProjectSort) : "manual");

/** A project nobody touched for this long goes under "Older". */
export const IDLE_AFTER_MS = 3 * 24 * 60 * 60 * 1000;
/** Activity is written at most this often per project, so output and keystrokes never cost a save. */
export const TOUCH_EVERY_MS = 60_000;

type Project = { id: string; name: string; parent?: string | null; addedAt?: number; pinned?: boolean };
export type ProjectRow<T> = { space: T; child: boolean };

/** `lastActive` with `ids` stamped `now`, or `lastActive` itself when each was stamped within a minute, so React skips the render. */
export function touched(lastActive: Record<string, number>, ids: string[], now: number): Record<string, number> {
  const stale = ids.filter((id) => !(now - (lastActive[id] ?? -Infinity) < TOUCH_EVERY_MS));
  return stale.length ? { ...lastActive, ...Object.fromEntries(stale.map((id) => [id, now])) } : lastActive;
}

/** Stamps projects that have no time yet `now`, so none goes under "Older" right after the first run with this. */
export function withMissing(lastActive: Record<string, number>, ids: string[], now: number): Record<string, number> {
  const missing = ids.filter((id) => !(id in lastActive));
  return missing.length ? { ...lastActive, ...Object.fromEntries(missing.map((id) => [id, now])) } : lastActive;
}

/** The saved times of projects that still exist; anything else in the file is ignored. */
export function restoreLastActive(saved: unknown, ids: string[]): Record<string, number> {
  if (!saved || typeof saved !== "object") return {};
  const known = new Set(ids);
  return Object.fromEntries(Object.entries(saved).filter(([id, t]) => known.has(id) && typeof t === "number" && Number.isFinite(t)));
}

/**
 * The projects in the order shown, worktrees right under their project, split into those listed and those under "Older".
 * Pinned projects come first, then `sort` decides. A project is older when it isn't pinned or active, runs no agents (`busy`,
 * its worktrees' included) and neither it nor a worktree was worked on for 3 days; one with no time yet is not older.
 */
export function orderProjects<T extends Project>(
  spaces: T[],
  o: { sort: ProjectSort; lastActive: Record<string, number>; now: number; activeId: string | null; busy: ReadonlySet<string> },
): { shown: ProjectRow<T>[]; older: ProjectRow<T>[] } {
  const groups: ProjectRow<T>[][] = [];
  for (const row of nestWorktrees(spaces)) {
    if (row.child) groups[groups.length - 1].push(row);
    else groups.push([row]);
  }
  const activity = (g: ProjectRow<T>[]) => {
    const times = g.flatMap((r) => (o.lastActive[r.space.id] === undefined ? [] : [o.lastActive[r.space.id]]));
    return times.length ? Math.max(...times) : undefined;
  };
  const by: Record<ProjectSort, (a: ProjectRow<T>[], b: ProjectRow<T>[]) => number> = {
    manual: () => 0,
    added: (a, b) => (b[0].space.addedAt ?? 0) - (a[0].space.addedAt ?? 0),
    worked: (a, b) => (activity(b) ?? 0) - (activity(a) ?? 0),
    name: (a, b) => a[0].space.name.localeCompare(b[0].space.name, undefined, { numeric: true, sensitivity: "base" }),
  };
  // Array.prototype.sort is stable, so ties (and projects with no time) keep their order.
  const sorted = [...groups].sort((a, b) => Number(!!b[0].space.pinned) - Number(!!a[0].space.pinned) || by[o.sort](a, b));
  const isOlder = (g: ProjectRow<T>[]) => {
    const last = activity(g);
    return !g[0].space.pinned && last !== undefined && o.now - last > IDLE_AFTER_MS && !g.some((r) => o.busy.has(r.space.id) || r.space.id === o.activeId);
  };
  return { shown: sorted.filter((g) => !isOlder(g)).flat(), older: sorted.filter(isOlder).flat() };
}
