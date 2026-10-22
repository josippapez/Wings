// "Elsewhere on this Mac": repos outside your projects that have worktrees. A scan asks Wings for every `.git` on every disk,
// asks git which of those repos have more than one worktree, and keeps their folders in storage, so they are there after a
// restart. The page loads those folders like any other repo, with git only: the disks are walked again by a scan alone.
//
// These repos aren't your projects, so they may be someone else's, and a repo's own settings and hooks can make some git
// commands run programs (see lib/git.ts). Nothing here runs more than `worktree list`.

import { mapLimit } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";
import { setting } from "wings-plugin-sdk/settings";

import type { Repo } from "./git";
import { hasWorktrees, parseWorktrees } from "./worktrees";

/** What the last scan left: when it finished, in ms since 1970 (0 before the first), and the main folders of the repos it found with worktrees. */
export interface Elsewhere {
  at: number;
  folders: string[];
}

/** Anything else that is stored under the key is as good as no scan. */
export function checkElsewhere(saved: unknown): Elsewhere | undefined {
  const s = saved as Partial<Elsewhere> | null;
  if (!s || typeof s.at !== "number" || !Number.isFinite(s.at) || !Array.isArray(s.folders)) return undefined;
  return { at: s.at, folders: s.folders.filter((f): f is string => typeof f === "string") };
}

/** Declared once: the page reads what the last scan left and sets what a new one finds. */
export const elsewhere = setting<Elsewhere>("elsewhere", { default: { at: 0, folders: [] }, check: checkElsewhere });

/** The folder a `.git` is in: a repo's own, or a linked worktree's. */
export const folderOf = (gitPath: string) => gitPath.slice(0, gitPath.lastIndexOf("/")) || "/";

/** What a scan doesn't open. Packages are never a project of yours, and the Trash holds what you threw away. */
const SKIP = ["node_modules", ".Trash"];

/**
 * The folders of the repos and worktrees on every disk, found by looking for `.git` on each disk in turn. A disk that
 * is read-only still counts: the startup disk is, and it reaches your own folders. `onLooked` hears how many files and
 * folders have been looked at so far, over all the disks.
 */
export async function findRepoFolders(signal: AbortSignal, onLooked: (looked: number) => void): Promise<string[]> {
  const folders = new Set<string>();
  let before = 0;
  for (const disk of await wings.disks()) {
    const found = await wings.diskFind(disk.mount, { names: [".git"], skip: SKIP, signal, onProgress: (p) => onLooked(before + p.entries) });
    before += found.entries;
    for (const path of found.paths) folders.add(folderOf(path));
  }
  onLooked(before);
  return [...folders].sort();
}

/**
 * The main folders of the repos among `folders` that have worktrees to manage, once each however many of their
 * worktrees were found, so a page doesn't load hundreds of repos that have only their own checkout. A folder git
 * can't read is left out. Throws once `signal` aborts.
 */
export async function reposWithWorktrees(folders: readonly string[], signal: AbortSignal): Promise<string[]> {
  const roots = new Set<string>();
  await mapLimit(folders, 8, async (folder) => {
    if (signal.aborted) throw new Error("The scan was stopped");
    const list = parseWorktrees((await gitOk(folder, ["worktree", "list", "--porcelain"])) ?? "");
    if (hasWorktrees(list)) roots.add(list[0].path);
  });
  return [...roots].sort();
}

/** The repos to list as elsewhere: the ones with worktrees that aren't `above` already, which are your projects' and your panes'. */
export function elsewhereRepos(found: readonly Repo[], above: readonly Repo[]): Repo[] {
  const shown = new Set(above.map((r) => r.root));
  return found.filter((r) => !shown.has(r.root) && hasWorktrees(r.list));
}
