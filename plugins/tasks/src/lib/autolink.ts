// Links an issue finds by itself in the project's repository: commits whose message names its key, and a branch that does.
// Plain logic plus `scanProject`, which takes the exec function, so a test counts the calls it makes.

import type { Op, Store } from "./issues.ts";

export interface Commit {
  sha: string;
  subject: string;
  message: string;
}

/** The distinct issue keys of `prefix` a text names, uppercase, in the order they appear. `win-12` counts; `WIN-12a`, `AWIN-12` and `WIN-1` inside `WIN-12` don't. */
export function keysIn(text: string, prefix: string): string[] {
  const found = new Set<string>();
  for (const m of text.matchAll(new RegExp(`\\b${prefix}-(\\d{1,9})\\b`, "gi"))) found.add(`${prefix.toUpperCase()}-${m[1]}`);
  return [...found];
}

/** What `git log -z --format=%H%x1f%B` printed: records split by NUL, fields by the unit separator. */
export function parseLog(stdout: string): Commit[] {
  const commits: Commit[] = [];
  for (const record of stdout.split("\0")) {
    const at = record.indexOf("\x1f");
    if (at < 0) continue;
    const message = record.slice(at + 1).trim();
    commits.push({ sha: record.slice(0, at).trim(), subject: message.split("\n")[0], message });
  }
  return commits;
}

/** One `autolink` op per existing issue the commits name, oldest commit first (git prints newest first). */
export function commitOps(store: Store, commits: Commit[]): Op[] {
  const known = new Set(store.issues.map((i) => i.key));
  const byKey = new Map<string, { sha: string; subject: string }[]>();
  for (const c of [...commits].reverse()) {
    for (const key of keysIn(c.message, store.prefix)) {
      if (known.has(key)) byKey.set(key, [...(byKey.get(key) ?? []), { sha: c.sha, subject: c.subject }]);
    }
  }
  return [...byKey].map(([key, list]) => ({ kind: "autolink", key, commits: list }));
}

/** An `autolink` op for each branch that names an existing issue with no branch yet. Only the first key in a name counts. */
export function branchOps(store: Store, branches: string[]): Op[] {
  const ops = new Map<string, Op>();
  for (const branch of branches) {
    const key = keysIn(branch, store.prefix).find((k) => store.issues.some((i) => i.key === k));
    const issue = store.issues.find((i) => i.key === key);
    if (key && issue && !issue.links?.branch && !ops.has(key)) ops.set(key, { kind: "autolink", key, branch });
  }
  return [...ops.values()];
}

export type Exec = (program: string, args: string[], options: { cwd: string }) => Promise<{ code: number | null; stdout: string }>;

/** The most pane folders asked for their branch in one scan. */
export const MAX_BRANCH_CALLS = 8;
const trim = (path: string) => path.replace(/\/+$/, "");

/**
 * Looks for links for one project: one `git log` over all branches since `since` (ms), plus one `git branch --show-current`
 * for each distinct pane folder other than the project's own, whose branch Wings already tells us. Nothing runs for a
 * project with no issues.
 */
export async function scanProject(exec: Exec, project: { path: string; branch: string | null }, paneCwds: string[], store: Store, since: number): Promise<{ ops: Op[]; calls: number }> {
  if (!store.issues.length) return { ops: [], calls: 0 };
  let calls = 1;
  const log = await exec("git", ["log", "--all", "-z", "--max-count=300", `--since=${new Date(since).toISOString()}`, "--format=%H%x1f%B"], { cwd: project.path });
  const ops = log.code === 0 ? commitOps(store, parseLog(log.stdout)) : [];
  const branches = project.branch ? [project.branch] : [];
  const folders = [...new Set(paneCwds.map(trim))].filter((c) => c && c !== trim(project.path)).slice(0, MAX_BRANCH_CALLS);
  calls += folders.length;
  const results = await Promise.all(folders.map((cwd) => exec("git", ["branch", "--show-current"], { cwd }).catch(() => null)));
  for (const r of results) if (r?.code === 0 && r.stdout.trim()) branches.push(r.stdout.trim());
  return { ops: [...ops, ...branchOps(store, branches)], calls };
}
