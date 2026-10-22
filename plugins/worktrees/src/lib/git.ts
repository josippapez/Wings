// Everything that runs git for the Worktrees page. The only commands that change anything are
// `git worktree remove`, `git worktree prune` and `git branch -d`, and none is ever given a force flag.
//
// A repo's own settings and hooks can make some git commands run programs: `git status` and `git log` (a filter, the
// file system monitor, a signature check), `git worktree remove` (it checks for changes first) and `git branch -d` (a
// hook on the ref). `worktree list`, `rev-parse`, `branch --list` and `worktree prune` run none. A repo that isn't one of
// your projects, like one a scan found on the disk, may be someone else's, so it is loaded with `check: false`, which
// runs only the ones that run nothing.

import { errorMessage, mapLimit } from "wings-plugin-sdk/async";
import { defaultBranch, git, gitOk, gitRun, repoRoot } from "wings-plugin-sdk/git";

import { describe, goneBranches, lines, parseWorktrees, type Facts, type Probe, type Row, type Worktree } from "./worktrees";

/** One repo: its worktrees and what we know about each. */
export interface Repo {
  /** The main worktree's folder, which names the repo and is where its git commands run. */
  root: string;
  list: Worktree[];
  facts: Facts;
  probes: Map<string, Probe>;
  /** Whether `git status` and `git log` were run in its worktrees. They aren't in a repo outside your projects. */
  checked: boolean;
}

async function probe(wt: Worktree): Promise<Probe> {
  if (wt.prunable !== null || wt.bare) return { dirty: false, lastCommit: null };
  const [status, last] = await Promise.all([gitOk(wt.path, ["status", "--porcelain"]), gitOk(wt.path, ["log", "--max-count=1", "--format=%ct"])]);
  return { dirty: status === null ? null : status !== "", lastCommit: last && /^\d+$/.test(last) ? Number(last) : null };
}

/** Reads one repo from any folder in it. With `check: false` it runs no `git status` or `git log` in its worktrees. */
export async function loadRepo(from: string, { check = true } = {}): Promise<Repo> {
  const list = parseWorktrees(await git(from, ["worktree", "list", "--porcelain"]));
  const root = list[0]?.path ?? from;
  const def = await defaultBranch(root);
  const [tip, merged, gone, probed] = await Promise.all([
    def ? gitOk(root, ["rev-parse", "--verify", "--quiet", `${def}^{commit}`]) : null,
    def ? gitOk(root, ["branch", "--list", `--merged=${def}`, "--format=%(refname:short)"]) : null,
    gitOk(root, ["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"]),
    // A few at a time, so a repo with many worktrees doesn't start them all at once.
    check ? mapLimit(list, 6, probe) : [],
  ]);
  return {
    root,
    list,
    facts: { defaultBranch: def, defaultTip: tip, merged: lines(merged ?? ""), gone: goneBranches(gone ?? "") },
    probes: new Map(check ? list.map((wt, i) => [wt.path, probed[i]]) : []),
    checked: check,
  };
}

export const rowsOf = (repo: Repo, paneCwds: readonly string[]): Row[] => describe(repo.list, repo.facts, repo.probes, paneCwds, !repo.checked);

/** The repos holding these folders, one each however many of their worktrees are open. A folder git can't read is skipped. */
export async function loadRepos(cwds: readonly string[], options?: { check?: boolean }): Promise<{ repos: Repo[]; errors: string[] }> {
  const tops = [...new Set((await Promise.all(cwds.map(repoRoot))).filter((t): t is string => t !== null))];
  const repos = new Map<string, Repo>();
  const errors: string[] = [];
  await Promise.all(
    tops.map(async (top) => {
      try {
        const repo = await loadRepo(top, options);
        repos.set(repo.root, repo);
      } catch (e) {
        errors.push(`${top}: ${errorMessage(e)}`);
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
 * old, and a worktree may have gone dirty or opened in a pane since. `onlyMerged` also needs it still merged and
 * checked clean, which a repo loaded with `check: false` never is. `check` is how the repo was loaded.
 */
export async function removeWorktrees(root: string, paths: string[], opts: { deleteBranch: boolean; onlyMerged: boolean; check?: boolean }, paneCwds: () => readonly string[]): Promise<Outcome> {
  const out: Outcome = { removed: [], failed: [], branchErrors: [] };
  const fresh = await loadRepo(root, { check: opts.check });
  for (const path of paths) {
    const row = rowsOf(fresh, paneCwds()).find((r) => r.wt.path === path);
    if (!row) {
      out.failed.push({ path, error: "It is no longer listed." });
      continue;
    }
    if (!row.removable || (opts.onlyMerged && !row.bulk)) {
      out.failed.push({ path, error: row.why ?? (row.unchecked ? "Its changes weren't checked, so it isn't removed with the others." : "It is no longer merged.") });
      continue;
    }
    try {
      await git(fresh.root, ["worktree", "remove", "--", path]);
    } catch (e) {
      out.failed.push({ path, error: errorMessage(e) });
      continue;
    }
    out.removed.push(path);
    if (opts.deleteBranch && row.wt.branch) {
      try {
        await git(fresh.root, ["branch", "-d", row.wt.branch]);
      } catch (e) {
        out.branchErrors.push({ branch: row.wt.branch, error: errorMessage(e) });
      }
    }
  }
  return out;
}

/** Forgets the worktrees whose folders are gone. */
export async function prune(root: string): Promise<string> {
  const r = await gitRun(root, ["worktree", "prune", "--verbose"]);
  if (r.code !== 0) throw new Error(r.stderr.trim() || "git worktree prune failed");
  return (r.stderr + r.stdout).trim();
}
