// The tools Claude gets. The build writes them into `contributes.mcpTools` of the manifest, so they run in Node
// then: nothing here may touch `wings` while it loads.

import { branchLabel, git, gitOk, repoRoot } from "wings-plugin-sdk/git";
import { s, tool } from "wings-plugin-sdk/tools";

import { changes } from "./lib/changes";
import { LOG, parseLog } from "./lib/log";

/** Readable refs for Claude, like `git log --decorate` shows them. */
const refName = (ref: string) => ({ H: ref === "H:HEAD" ? "HEAD" : `HEAD -> ${ref.slice(2)}`, t: `tag: ${ref.slice(2)}` })[ref[0]] ?? ref.slice(2);

const folder = s.string().describe("An absolute path to a folder inside the repo.");

/** The root of the repo that contains `dir`. */
async function rootOf(dir: string) {
  const root = await repoRoot(dir);
  if (!root) throw new Error(`${dir} is not inside a git repo.`);
  return root;
}

export const tools = {
  changed_files: tool({
    description:
      "Lists the files changed in the git repo containing a folder, with each file's status (M modified, A added, D deleted, R renamed, ? untracked) and added and removed line counts. `against` is \"uncommitted\" (the working tree, staged and untracked included, against HEAD; the default) or \"branch\" (everything since this branch left the default branch). Read-only.",
    input: s.object({ cwd: folder, against: s.enum(["uncommitted", "branch"]).optional() }),
    async run({ cwd, against = "uncommitted" }) {
      const c = await changes(await rootOf(cwd), against, { fresh: true });
      return {
        repo: c.root,
        branch: c.branch,
        against: c.against,
        added: c.added,
        removed: c.removed,
        total: c.files.length,
        files: c.files.slice(0, 500).map(({ path, oldPath, status, added, removed }) => ({ path, ...(oldPath ? { oldPath } : {}), status, added, removed })),
      };
    },
  }),

  recent_commits: tool({
    description:
      "Lists the newest commits on the current branch of the git repo containing a folder: hash, subject, author, ISO date, refs (branches and tags pointing at it) and whether it's a merge. `limit` is 1 to 200, 20 by default. Read-only.",
    input: s.object({ cwd: folder, limit: s.integer({ minimum: 1, maximum: 200 }).optional() }),
    async run({ cwd, limit = 20 }) {
      const root = await rootOf(cwd);
      const head = await gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"]);
      if (!head) return { repo: root, commits: [] };
      const commits = parseLog(await git(root, ["log", ...LOG, `--max-count=${limit}`], { raw: true }));
      return {
        repo: root,
        branch: branchLabel(await gitOk(root, ["branch", "--show-current"]), head),
        commits: commits.map((c) => ({ hash: c.hash, subject: c.subject, author: c.author, date: new Date(c.at * 1000).toISOString(), refs: c.refs.map(refName), ...(c.parents.length > 1 ? { merge: true } : {}) })),
      };
    },
  }),
};
