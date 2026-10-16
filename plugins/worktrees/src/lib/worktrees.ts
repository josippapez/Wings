// The logic of the Worktrees page that needs no git or DOM: reading `git worktree list --porcelain`, deciding what
// each worktree is, and which ones may be removed. Kept pure so `worktrees.test.mjs` runs it in Node.

export interface Worktree {
  path: string;
  head: string | null;
  /** The short branch name, null when detached or bare. */
  branch: string | null;
  bare: boolean;
  detached: boolean;
  /** The lock reason; "" when locked without one, null when not locked. */
  locked: string | null;
  /** Why git would prune it (its folder is gone); null when it wouldn't. */
  prunable: string | null;
  /** The first entry of the list: the repo's own checkout. */
  main: boolean;
}

/** What `git worktree list --porcelain` prints: blocks of `key value` lines separated by an empty line. */
export function parseWorktrees(text: string): Worktree[] {
  const out: Worktree[] = [];
  for (const block of text.split(/\r?\n\r?\n/)) {
    const wt: Worktree = { path: "", head: null, branch: null, bare: false, detached: false, locked: null, prunable: null, main: false };
    for (const line of block.split(/\r?\n/)) {
      const space = line.indexOf(" ");
      const key = space < 0 ? line : line.slice(0, space);
      const value = space < 0 ? "" : line.slice(space + 1);
      if (key === "worktree") wt.path = value;
      else if (key === "HEAD") wt.head = value;
      else if (key === "branch") wt.branch = value.replace(/^refs\/heads\//, "");
      else if (key === "bare") wt.bare = true;
      else if (key === "detached") wt.detached = true;
      else if (key === "locked") wt.locked = value;
      else if (key === "prunable") wt.prunable = value;
    }
    if (wt.path) out.push(wt);
  }
  if (out[0]) out[0].main = true;
  return out;
}

/** What `git status` and `git log` said about one worktree. */
export interface Probe {
  /** null when git couldn't say. */
  dirty: boolean | null;
  /** The newest commit's time in seconds, null when unknown. */
  lastCommit: number | null;
}

/** What one repo says about its branches. */
export interface Facts {
  /** The default branch as a ref, like `origin/main`. */
  defaultBranch: string | null;
  /** The commit the default branch is at. */
  defaultTip: string | null;
  /** Local branches merged into the default branch. */
  merged: ReadonlySet<string>;
  /** Local branches whose upstream is gone. */
  gone: ReadonlySet<string>;
}

export interface Row {
  wt: Worktree;
  missing: boolean;
  /** The branch is merged into the default branch and has commits of its own on it. */
  merged: boolean;
  /** The branch sits exactly on the default branch's commit: new, or merged by fast-forward. */
  atDefault: boolean;
  gone: boolean;
  dirty: boolean | null;
  locked: boolean;
  inPane: boolean;
  lastCommit: number | null;
  /** `git worktree remove` (without --force) can be offered. */
  removable: boolean;
  /** Why not, for the tooltip. */
  why: string | null;
  /** Part of "Remove all merged & clean". */
  bulk: boolean;
}

const trim = (p: string) => (p.length > 1 ? p.replace(/\/+$/, "") : p);
/** Whether `path` is `root` or inside it. */
export const contains = (root: string, path: string) => {
  const r = trim(root);
  const p = trim(path);
  return p === r || p.startsWith(r === "/" ? "/" : `${r}/`);
};

/** The default branch's own name, `main` for `origin/main`. */
const localName = (ref: string) => ref.replace(/^origin\//, "");

/** Decides what each worktree is. A pane belongs to the deepest worktree that holds its folder. */
export function describe(list: Worktree[], facts: Facts, probes: ReadonlyMap<string, Probe>, paneCwds: readonly string[]): Row[] {
  const open = new Set<string>();
  for (const cwd of paneCwds) {
    let best: Worktree | null = null;
    for (const wt of list) if (contains(wt.path, cwd) && (!best || trim(wt.path).length > trim(best.path).length)) best = wt;
    if (best) open.add(best.path);
  }
  const defaultName = facts.defaultBranch ? localName(facts.defaultBranch) : null;
  return list.map((wt) => {
    const probe = probes.get(wt.path);
    const missing = wt.prunable !== null;
    const inDefault = !wt.main && wt.branch !== null && wt.branch !== defaultName && facts.merged.has(wt.branch);
    const atDefault = inDefault && wt.head !== null && wt.head === facts.defaultTip;
    const merged = inDefault && !atDefault;
    const dirty = missing || wt.bare ? false : (probe?.dirty ?? null);
    const locked = wt.locked !== null;
    const inPane = open.has(wt.path);
    const why = wt.main ? "The main worktree stays." : locked ? "Locked." : missing ? "Its folder is gone: prune it." : inPane ? "Open in a pane." : dirty === null ? "Couldn't read its status." : dirty ? "Has uncommitted changes." : null;
    const removable = why === null;
    return {
      wt,
      missing,
      merged,
      atDefault,
      gone: wt.branch !== null && facts.gone.has(wt.branch),
      dirty,
      locked,
      inPane,
      lastCommit: probe?.lastCommit ?? null,
      removable,
      why,
      bulk: removable && merged,
    };
  });
}

/** `/Users/me/dev/app` as `~/dev/app`. */
export function shortenPath(path: string): string {
  return path.replace(/^(?:\/Users|\/home)\/[^/]+(?=\/|$)|^\/root(?=\/|$)/, "~");
}

/** The lines of `git branch --format=%(refname:short)%09%(upstream:track)` whose upstream is gone. */
export function goneBranches(text: string): Set<string> {
  const out = new Set<string>();
  for (const line of text.split("\n")) {
    const [name, track] = line.split("\t");
    if (name && track?.trim() === "[gone]") out.add(name.trim());
  }
  return out;
}

/** One branch name per line. */
export const lines = (text: string) => new Set(text.split("\n").map((l) => l.trim()).filter(Boolean));
