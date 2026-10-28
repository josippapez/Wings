// ---------- a changed file's diff, for a row of Files changed ----------

import { openDiffWhile } from "wings-plugin-sdk/async";
import { basename } from "wings-plugin-sdk/format";

import type { FileChange, Issue } from "./domain";

/** Matches the journal's `changed (+5 -2)`, for the row's counts. */
const COUNTS = /^changed \(\+(\d+) -(\d+)\)$/;

/** The added and removed line counts in a journal's `what`, or null when it says something else. */
export function lineCounts(what: string): { added: number; removed: number } | null {
  const m = COUNTS.exec(what.trim());
  return m ? { added: Number(m[1]), removed: Number(m[2]) } : null;
}

/** The git diff for one file of the issue: from before its first linked commit to the working tree, or against HEAD without one. */
export function diffArgs(issue: Pick<Issue, "links">, path: string): string[] {
  const first = issue.links?.commits?.[0];
  return ["diff", first ? `${first}^` : "HEAD", "--no-color", "--no-ext-diff", "--", path];
}

/**
 * Opens the diff viewer at once and fills it when git answers, so a click never waits. A path from the journal can't
 * lead out of the project: one with `..` or that starts at the root is refused.
 */
export function openFileDiff(issue: Issue, file: FileChange, root: string) {
  if (file.path.startsWith("/") || file.path.split("/").includes("..")) throw new Error("That file is outside the project, so there is no diff to show.");
  const first = issue.links?.commits?.[0];
  const subtitle = first ? `Since before ${first.slice(0, 7)}, with what isn't committed yet` : "Working tree against HEAD";
  return openDiffWhile({ title: basename(file.path), subtitle, cwd: root }, async () => {
    let r = await wings.exec("git", diffArgs(issue, file.path), { cwd: root });
    // A commit that's gone, like after a rebase, leaves the working tree against HEAD.
    if (r.code !== 0 && first) r = await wings.exec("git", diffArgs({}, file.path), { cwd: root });
    if (r.code !== 0) throw new Error(r.stderr.trim() || "git diff failed");
    return r.stdout.trim() ? { patch: r.stdout } : { error: "No changes to show: it may be committed already before this issue, or not tracked yet." };
  });
}
