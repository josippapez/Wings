import { openDiffWhile } from "wings-plugin-sdk/async";
import { plural } from "wings-plugin-sdk/format";
import { git, gitOk } from "wings-plugin-sdk/git";

import { dateTimeOf } from "./dates";
import { MAX_PATCH_BYTES, MAX_PATCH_FILES, MAX_PATCH_LINES } from "./diff";
import { COLOR } from "./git";
import { LOG, parseLog, type Commit } from "./log";
import type { CommitRef, CommitRow } from "./types";

/** Commits the History tab loads at a time. */
const PAGE = 50;

/**
 * One page of the current branch's commits, or every branch's, newest first. A search matches the message, the
 * author or the start of a hash: git ANDs `--grep` with `--author`, so they're two lookups merged by date.
 */
export async function historyPage(root: string, { query, all, skip }: { query: string; all: boolean; skip: number }) {
  const scope = all ? ["--all"] : [];
  if (!query) {
    const commits = parseLog(await git(root, ["log", ...LOG, `--max-count=${PAGE + 1}`, `--skip=${skip}`, ...scope], { raw: true }));
    return { commits: commits.slice(0, PAGE), more: commits.length > PAGE };
  }
  const match = [...LOG, "--regexp-ignore-case", "--fixed-strings", `--max-count=${skip + PAGE + 1}`];
  const hash = /^[0-9a-f]{4,40}$/i.test(query) ? await gitOk(root, ["rev-parse", "--verify", "--quiet", `${query}^{commit}`]) : null;
  const [byMessage, byAuthor, byHash] = await Promise.all([
    git(root, ["log", ...match, `--grep=${query}`, ...scope], { raw: true }),
    git(root, ["log", ...match, `--author=${query}`, ...scope], { raw: true }),
    hash ? git(root, ["log", ...LOG, "--max-count=1", hash, "--"], { raw: true }) : "",
  ]);
  const seen = new Set<string>();
  const merged = [...parseLog(byHash), ...[...parseLog(byMessage), ...parseLog(byAuthor)].sort((a, b) => b.ct - a.ct)].filter((c) => !seen.has(c.hash) && seen.add(c.hash));
  return { commits: merged.slice(skip, skip + PAGE), more: merged.length > skip + PAGE };
}

/** A commit as the sidebar's answer holds it. */
export const commitRow = (c: Commit): CommitRow => [c.hash, c.subject.slice(0, 200), c.author.slice(0, 80), c.at, c.refs.slice(0, 6).map((r) => r.slice(0, 60))];

/** `git show` for one commit; a merge against its first parent, so it shows what the merge brought in. */
const SHOW = ["show", "--format=", "--diff-merges=first-parent", "-M", ...COLOR];

/** Files and changed lines in `git show --numstat -z`. */
function numstatSize(numstat: string) {
  let files = 0;
  let size = 0;
  const s = numstat.split("\0");
  for (let i = 0; i < s.length && s[i]; ) {
    const [added, removed, ...rest] = s[i].split("\t");
    // A rename has an empty path, then its old and new paths as their own fields.
    i += rest.join("\t") ? 1 : 3;
    files += 1;
    size += (Number(added) || 0) + (Number(removed) || 0);
  }
  return { files, size };
}

/**
 * Opens the viewer at once with what the page already showed, then fills it with the commit's patch.
 */
export function openCommit(root: string, { hash, subject, author, at }: Omit<CommitRef, "root">) {
  const short = hash.slice(0, 7);
  return openDiffWhile({ title: subject || short, subtitle: [short, author, at ? dateTimeOf(at) : ""].filter(Boolean).join(" · ") }, async () => {
    const { files, size } = numstatSize(await git(root, [...SHOW, "--numstat", "-z", hash, "--"], { raw: true }));
    if (files === 0) return { error: "This commit changes no files." };
    if (files > MAX_PATCH_FILES || size > MAX_PATCH_LINES) throw new Error(`Too large to show: ${plural(files, "file")}, ${plural(size, "changed line")}.`);
    const patch = await git(root, [...SHOW, "--patch", hash, "--"], { raw: true });
    if (patch.length > MAX_PATCH_BYTES) throw new Error(`Too large to show: ${Math.round(patch.length / 1024 / 1024)} MB of changes in ${plural(files, "file")}.`);
    return { patch };
  });
}
