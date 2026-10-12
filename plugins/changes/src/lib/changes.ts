import { basename, COLOR, defaultBranch, EMPTY_TREE, git, gitOk, literal } from "./git";
import type { Mode } from "./types";

/** A result this fresh is reused, so the badge poll and the sidebar's refresh don't run git twice. */
const FRESH_MS = 3_000;
/** Untracked files read through `git grep`, which takes their paths as arguments. */
export const MAX_UNTRACKED_READ = 200;

/**
 * `added` and `removed` are null for binary files, and `added` for untracked files git couldn't read as text.
 */
export interface FileChange {
  path: string;
  oldPath?: string;
  status: string;
  added: number | null;
  removed: number | null;
  untracked?: boolean;
}

export interface Changes {
  root: string;
  name: string;
  branch: string;
  mode: Mode;
  base: string;
  baseLabel: string;
  files: FileChange[];
  added: number;
  removed: number;
  partial: boolean;
  at: number;
}

/** Changes by `${root}\0${mode}`, and the lookups still running. */
const cache = new Map<string, Changes>();
const running = new Map<string, Promise<Changes>>();

/** Splits `git diff --numstat -z` and `--name-status -z` into one entry per file. */
function parseDiff(numstat: string, names: string) {
  const files = new Map<string, FileChange>();
  const n = names.split("\0");
  for (let i = 0; i < n.length && n[i]; ) {
    const status = n[i];
    const two = status[0] === "R" || status[0] === "C";
    const file = two ? { path: n[i + 2], oldPath: n[i + 1], status: status[0] } : { path: n[i + 1], status: status[0] };
    files.set(file.path, { ...file, added: 0, removed: 0 });
    i += two ? 3 : 2;
  }
  const s = numstat.split("\0");
  for (let i = 0; i < s.length && s[i]; ) {
    const [added, removed, ...rest] = s[i].split("\t");
    const path = rest.join("\t") || s[i + 2];
    i += rest.join("\t") ? 1 : 3;
    const file = files.get(path);
    if (!file) continue;
    file.added = added === "-" ? null : Number(added);
    file.removed = removed === "-" ? null : Number(removed);
  }
  return [...files.values()];
}

/** Line counts of untracked text files, from `git grep --count`; binary and empty files are left out. */
async function lineCounts(root: string, paths: string[]) {
  const counts = new Map<string, number>();
  if (paths.length === 0) return counts;
  const r = await wings.exec("git", ["grep", "--untracked", "-I", "-z", "--count", "--no-color", "-e", "", "--", ...literal(paths)], { cwd: root });
  // 1 is "no match": every file was empty or binary.
  if (r.code !== null && r.code > 1) throw new Error(r.stderr.trim() || "git grep failed");
  for (const line of r.stdout.split("\n")) {
    const at = line.lastIndexOf("\0");
    if (at > 0) counts.set(line.slice(0, at), Number(line.slice(at + 1)));
  }
  return counts;
}

/** The changed files of a repo: against HEAD, or against where this branch left the default branch. */
async function load(root: string, mode: Mode): Promise<Changes> {
  const [branch, head] = await Promise.all([gitOk(root, ["branch", "--show-current"]), gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"])]);
  let base = head ?? EMPTY_TREE;
  let baseLabel = head ? "HEAD" : "an empty repo";
  if (mode === "branch") {
    const ref = await defaultBranch(root);
    if (!ref) throw new Error("No default branch found (origin/HEAD, main or master).");
    if (!head) throw new Error("This repo has no commits yet.");
    base = (await git(root, ["merge-base", "HEAD", ref])).trim();
    baseLabel = `${ref} at ${base.slice(0, 7)}`;
  }
  const [numstat, names, others] = await Promise.all([
    git(root, ["diff", base, "--numstat", "-z", "-M", ...COLOR]),
    git(root, ["diff", base, "--name-status", "-z", "-M", ...COLOR]),
    git(root, ["ls-files", "--others", "--exclude-standard", "-z"]),
  ]);
  const files = parseDiff(numstat, names);
  const untracked = others.split("\0").filter(Boolean);
  const counts = await lineCounts(root, untracked.slice(0, MAX_UNTRACKED_READ));
  for (const path of untracked) files.push({ path, status: "?", added: counts.get(path) ?? null, removed: 0, untracked: true });
  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return {
    root,
    name: basename(root),
    branch: branch || (head ? `detached at ${head.slice(0, 7)}` : "no commits"),
    mode,
    base,
    baseLabel,
    files,
    added: files.reduce((n, f) => n + (f.added ?? 0), 0),
    removed: files.reduce((n, f) => n + (f.removed ?? 0), 0),
    partial: untracked.length > MAX_UNTRACKED_READ,
    at: Date.now(),
  };
}

/** Cached for a moment, and one lookup at a time per repo and mode. */
export function changes(root: string, mode: Mode, { fresh = false }: { fresh?: boolean } = {}) {
  const key = `${root}\0${mode}`;
  const hit = cache.get(key);
  if (!fresh && hit && Date.now() - hit.at < FRESH_MS) return Promise.resolve(hit);
  if (running.has(key)) return running.get(key)!;
  const p = load(root, mode).then(
    (c) => (cache.set(key, c), c),
    (error) => {
      cache.delete(key);
      throw error;
    },
  );
  running.set(key, p);
  void p.finally(() => running.delete(key)).catch(() => {});
  return p;
}
