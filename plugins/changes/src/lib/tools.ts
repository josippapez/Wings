import { changes } from "./changes";
import { against } from "./diff";
import { git, gitOk, rootOf } from "./git";
import { LOG, parseLog } from "./log";

/** Readable refs for Claude, like `git log --decorate` shows them. */
const refName = (ref: string) => ({ H: ref === "H:HEAD" ? "HEAD" : `HEAD -> ${ref.slice(2)}`, t: `tag: ${ref.slice(2)}` })[ref[0]] ?? ref.slice(2);

export async function recentCommits(input: Record<string, unknown>) {
  const cwd = typeof input?.cwd === "string" ? input.cwd : null;
  if (!cwd) throw new Error("cwd is required: a folder inside a git repo.");
  const root = await rootOf(cwd);
  if (!root) throw new Error(`${cwd} is not inside a git repo.`);
  const limit = Math.min(200, Math.max(1, Math.floor(Number(input?.limit)) || 20));
  if (!(await gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"]))) return { repo: root, commits: [] };
  const commits = parseLog(await git(root, ["log", ...LOG, `--max-count=${limit}`]));
  return {
    repo: root,
    branch: (await gitOk(root, ["branch", "--show-current"])) || "detached HEAD",
    commits: commits.map((c) => ({ hash: c.hash, subject: c.subject, author: c.author, date: new Date(c.at * 1000).toISOString(), refs: c.refs.map(refName), ...(c.parents.length > 1 ? { merge: true } : {}) })),
  };
}

export async function changedFiles(input: Record<string, unknown>) {
  const cwd = typeof input?.cwd === "string" ? input.cwd : null;
  if (!cwd) throw new Error("cwd is required: a folder inside a git repo.");
  const root = await rootOf(cwd);
  if (!root) throw new Error(`${cwd} is not inside a git repo.`);
  const mode = input?.against === "branch" ? "branch" : "uncommitted";
  const c = await changes(root, mode, { fresh: true });
  return {
    repo: c.root,
    branch: c.branch,
    against: against(mode, c.baseLabel),
    added: c.added,
    removed: c.removed,
    total: c.files.length,
    files: c.files.slice(0, 500).map(({ path, oldPath, status, added, removed }) => ({ path, ...(oldPath ? { oldPath } : {}), status, added, removed })),
  };
}
