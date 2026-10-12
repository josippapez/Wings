// Everything that runs git for the Worktrees page. The only commands that change anything are
// `git worktree remove`, `git worktree prune` and `git branch -d`, and none is ever given a force flag.

import { describe, goneBranches, lines, parseWorktrees, type Facts, type Probe, type Row, type Worktree } from "./worktrees";

/** One repo: its worktrees and what we know about each. */
export interface Repo {
  /** The main worktree's folder, which names the repo and is where its git commands run. */
  root: string;
  list: Worktree[];
  facts: Facts;
  probes: Map<string, Probe>;
}

const run = (cwd: string, args: string[]) => wings.exec("git", args, { cwd });
/** Stdout, or git's own message when it failed. */
async function git(cwd: string, args: string[]): Promise<string> {
  const r = await run(cwd, args);
  if (r.code !== 0) throw new Error(r.stderr.trim() || `git ${args.slice(0, 2).join(" ")} failed`);
  return r.stdout;
}
const gitOk = (cwd: string, args: string[]) => run(cwd, args).then((r) => (r.code === 0 ? r.stdout.trim() : null), () => null);

/** Runs `fn` over `items`, a few at a time, so a repo with many worktrees doesn't start them all at once. */
async function pool<T, R>(items: T[], size: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(size, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i]);
      }
    }),
  );
  return out;
}

/** The checkout a folder is in; null outside a repo. Cached, since a pane's folder rarely changes repo. */
const tops = new Map<string, Promise<string | null>>();
export const topOf = (cwd: string) => {
  if (!tops.has(cwd)) tops.set(cwd, gitOk(cwd, ["rev-parse", "--show-toplevel"]).then((r) => r || null));
  return tops.get(cwd)!;
};

/** `origin/HEAD` as the remote set it, else the usual names. */
async function defaultBranch(root: string): Promise<string | null> {
  let ref = await gitOk(root, ["rev-parse", "--abbrev-ref", "origin/HEAD"]);
  if (!ref || ref === "origin/HEAD") ref = null;
  for (const name of ["origin/main", "origin/master", "main", "master"]) {
    if (ref) break;
    if (await gitOk(root, ["rev-parse", "--verify", "--quiet", `${name}^{commit}`])) ref = name;
  }
  return ref;
}

async function probe(wt: Worktree): Promise<Probe> {
  if (wt.prunable !== null || wt.bare) return { dirty: false, lastCommit: null };
  const [status, last] = await Promise.all([gitOk(wt.path, ["status", "--porcelain"]), gitOk(wt.path, ["log", "--max-count=1", "--format=%ct"])]);
  return { dirty: status === null ? null : status !== "", lastCommit: last && /^\d+$/.test(last) ? Number(last) : null };
}

/** Reads one repo from any folder in it. */
export async function loadRepo(from: string): Promise<Repo> {
  const list = parseWorktrees(await git(from, ["worktree", "list", "--porcelain"]));
  const root = list[0]?.path ?? from;
  const def = await defaultBranch(root);
  const [tip, merged, gone, probed] = await Promise.all([
    def ? gitOk(root, ["rev-parse", "--verify", "--quiet", `${def}^{commit}`]) : null,
    def ? gitOk(root, ["branch", "--list", `--merged=${def}`, "--format=%(refname:short)"]) : null,
    gitOk(root, ["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"]),
    pool(list, 6, probe),
  ]);
  return {
    root,
    list,
    facts: { defaultBranch: def, defaultTip: tip, merged: lines(merged ?? ""), gone: goneBranches(gone ?? "") },
    probes: new Map(list.map((wt, i) => [wt.path, probed[i]])),
  };
}

export const rowsOf = (repo: Repo, paneCwds: readonly string[]): Row[] => describe(repo.list, repo.facts, repo.probes, paneCwds);

/** The repos holding these folders, one each however many of their worktrees are open. A folder git can't read is skipped. */
export async function loadRepos(cwds: readonly string[]): Promise<{ repos: Repo[]; errors: string[] }> {
  const tops = [...new Set((await Promise.all(cwds.map(topOf))).filter((t): t is string => t !== null))];
  const repos = new Map<string, Repo>();
  const errors: string[] = [];
  await Promise.all(
    tops.map(async (top) => {
      try {
        const repo = await loadRepo(top);
        repos.set(repo.root, repo);
      } catch (e) {
        errors.push(`${top}: ${(e as Error).message}`);
      }
    }),
  );
  return { repos: [...repos.values()].sort((a, b) => a.root.localeCompare(b.root)), errors };
}

export interface Outcome {
  removed: string[];
  failed: { path: string; error: string }[];
  /** Branches `git branch -d` refused or couldn't delete. */
  branchErrors: { branch: string; error: string }[];
  pruned?: string;
}

/**
 * Removes the worktrees at `paths`, each only if it is still removable right now: the page may be minutes
 * old, and a worktree may have gone dirty or opened in a pane since. `onlyMerged` also needs it still merged.
 */
export async function removeWorktrees(root: string, paths: string[], opts: { deleteBranch: boolean; onlyMerged: boolean }, paneCwds: () => readonly string[]): Promise<Outcome> {
  const out: Outcome = { removed: [], failed: [], branchErrors: [] };
  const fresh = await loadRepo(root);
  for (const path of paths) {
    const row = rowsOf(fresh, paneCwds()).find((r) => r.wt.path === path);
    if (!row) {
      out.failed.push({ path, error: "It is no longer listed." });
      continue;
    }
    if (!row.removable || (opts.onlyMerged && !row.merged)) {
      out.failed.push({ path, error: row.why ?? "It is no longer merged." });
      continue;
    }
    const r = await run(fresh.root, ["worktree", "remove", "--", path]).catch((e: Error) => ({ code: 1, stdout: "", stderr: e.message }));
    if (r.code !== 0) {
      out.failed.push({ path, error: r.stderr.trim() || "git worktree remove failed" });
      continue;
    }
    out.removed.push(path);
    if (opts.deleteBranch && row.wt.branch) {
      const b = await run(fresh.root, ["branch", "-d", row.wt.branch]).catch((e: Error) => ({ code: 1, stdout: "", stderr: e.message }));
      if (b.code !== 0) out.branchErrors.push({ branch: row.wt.branch, error: b.stderr.trim() || "git branch -d failed" });
    }
  }
  return out;
}

/** Forgets the worktrees whose folders are gone. */
export async function prune(root: string): Promise<string> {
  const r = await run(root, ["worktree", "prune", "--verbose"]);
  if (r.code !== 0) throw new Error(r.stderr.trim() || "git worktree prune failed");
  return (r.stderr + r.stdout).trim();
}
