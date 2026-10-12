import { plural } from "./format";
import { MAX_PATCH_BYTES, MAX_PATCH_FILES, MAX_PATCH_LINES } from "./diff";
import { COLOR, git, gitOk } from "./git";
import { LOG, parseLog, type Commit } from "./log";
import type { CommitRow } from "./types";

/** Commits the History tab loads at a time. */
const PAGE = 50;
/** What one broadcast to the sidebar may spend on files, under Wings' 64 KB limit. */
export const MAX_LIST_BYTES = 56_000;

/**
 * One page of the current branch's commits, or every branch's, newest first. A search matches the message, the
 * author or the start of a hash: git ANDs `--grep` with `--author`, so they're two lookups merged by date.
 */
export async function historyPage(root: string, { query, all, skip }: { query: string; all: boolean; skip: number }) {
  const scope = all ? ["--all"] : [];
  if (!query) {
    const commits = parseLog(await git(root, ["log", ...LOG, `--max-count=${PAGE + 1}`, `--skip=${skip}`, ...scope]));
    return { commits: commits.slice(0, PAGE), more: commits.length > PAGE };
  }
  const match = [...LOG, "--regexp-ignore-case", "--fixed-strings", `--max-count=${skip + PAGE + 1}`];
  const hash = /^[0-9a-f]{4,40}$/i.test(query) ? await gitOk(root, ["rev-parse", "--verify", "--quiet", `${query}^{commit}`]) : null;
  const [byMessage, byAuthor, byHash] = await Promise.all([
    git(root, ["log", ...match, `--grep=${query}`, ...scope]),
    git(root, ["log", ...match, `--author=${query}`, ...scope]),
    hash ? git(root, ["log", ...LOG, "--max-count=1", hash, "--"]) : "",
  ]);
  const seen = new Set<string>();
  const merged = [...parseLog(byHash), ...[...parseLog(byMessage), ...parseLog(byAuthor)].sort((a, b) => b.ct - a.ct)].filter((c) => !seen.has(c.hash) && seen.add(c.hash));
  return { commits: merged.slice(skip, skip + PAGE), more: merged.length > skip + PAGE };
}

/** Commits as `[hash, subject, author, at, refs]`, as many as fit in one broadcast. */
export function compactCommits(commits: Commit[]) {
  const out: CommitRow[] = [];
  let bytes = 0;
  for (const c of commits) {
    const row: CommitRow = [c.hash, c.subject.slice(0, 200), c.author.slice(0, 80), c.at, c.refs.slice(0, 6).map((r) => r.slice(0, 60))];
    bytes += JSON.stringify(row).length + 1;
    if (bytes > MAX_LIST_BYTES) break;
    out.push(row);
  }
  return out;
}

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
export async function openCommit(root: string, { hash, subject, author, at }: { hash: string; subject?: string; author?: string; at?: number }) {
  const short = hash.slice(0, 7);
  const date = at ? new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "";
  const { id } = await wings.openDiff({ title: subject || short, subtitle: [short, author, date].filter(Boolean).join(" · ") });
  try {
    const { files, size } = numstatSize(await git(root, [...SHOW, "--numstat", "-z", hash, "--"]));
    if (files === 0) return void (await wings.updateDiff(id, { error: "This commit changes no files." }));
    if (files > MAX_PATCH_FILES || size > MAX_PATCH_LINES) throw new Error(`Too large to show: ${plural(files, "file")}, ${plural(size, "changed line")}.`);
    const patch = await git(root, [...SHOW, "--patch", hash, "--"]);
    if (patch.length > MAX_PATCH_BYTES) throw new Error(`Too large to show: ${Math.round(patch.length / 1024 / 1024)} MB of changes in ${plural(files, "file")}.`);
    await wings.updateDiff(id, { patch });
  } catch (error) {
    await wings.updateDiff(id, { error: String((error as Error).message ?? error) });
  }
}
