import { openDiffWhile } from "wings-plugin-sdk/async";
import { basename, plural } from "wings-plugin-sdk/format";
import { git, gitRun } from "wings-plugin-sdk/git";

import { batched, fitPatch, placeholder } from "./batch";
import { changes, MAX_UNTRACKED_READ, type Changes, type FileChange } from "./changes";
import { COLOR, literal } from "./git";
import type { Mode } from "./types";

/** About 2 MB of patch in all. Past this the biggest files turn into "Too large to show" entries, since the viewer would take seconds to draw them. */
export const MAX_PATCH_LINES = 50_000;
export const MAX_PATCH_BYTES = 2 * 1024 * 1024;
export const MAX_PATCH_FILES = 1_500;

/** A patch for files git doesn't track yet, read with `git grep`, since Wings refuses `git diff --no-index`. */
async function untrackedPatch(root: string, files: FileChange[]) {
  if (files.length === 0) return "";
  const paths = files.map((f) => f.path);
  const r = await gitRun(root, ["grep", "--untracked", "-I", "-z", "--no-color", "--no-line-number", "--no-column", "-e", "", "--", ...literal(paths)]);
  if (r.code !== null && r.code > 1) throw new Error(r.stderr.trim() || "git grep failed");
  const text = new Map<string, string[]>();
  for (const line of r.stdout.split("\n")) {
    const at = line.indexOf("\0");
    if (at < 0) continue;
    const path = line.slice(0, at);
    if (!text.has(path)) text.set(path, []);
    text.get(path)?.push(line.slice(at + 1));
  }
  // Files with no text lines are empty or binary; counting without -I tells them apart.
  const rest = paths.filter((p) => !text.has(p));
  const binary = new Set<string>();
  if (rest.length) {
    const all = await gitRun(root, ["grep", "--untracked", "-z", "--count", "--no-color", "-e", "", "--", ...literal(rest)]);
    for (const line of all.stdout.split("\n")) if (line.includes("\0")) binary.add(line.slice(0, line.lastIndexOf("\0")));
  }
  return paths
    .map((path) => {
      const head = `diff --git a/${path} b/${path}\nnew file mode 100644\n`;
      if (binary.has(path)) return `${head}Binary files /dev/null and b/${path} differ\n`;
      const body = text.get(path);
      if (!body) return head;
      return `${head}--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${body.length} @@\n${body.map((l) => `+${l}\n`).join("")}`;
    })
    .join("");
}

/** The patch for one file, or all of them. */
async function patchOf(c: Changes, path: string | null) {
  const files = path == null ? c.files : c.files.filter((f) => f.path === path);
  if (path != null && files.length === 0) throw new Error("This file has no changes any more.");
  const size = files.reduce((n, f) => n + (f.added ?? 0) + (f.removed ?? 0), 0);
  if (files.length > MAX_PATCH_FILES || size > MAX_PATCH_LINES) {
    throw new Error(`Too large to show: ${plural(files.length, "file")}, ${plural(size, "changed line")}. Open files one at a time from the Changes sidebar.`);
  }
  const tracked = files.filter((f) => !f.untracked);
  const untracked = files.filter((f) => f.untracked);
  // Never one giant patch: git's output is capped at 8 MB, and one minified bundle can be most of it.
  const tooBig = (path: string) => placeholder(`diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n`, "Too large to show (over 8 MB)");
  const [diff, added] = await Promise.all([
    batched(
      tracked,
      (part) => git(c.root, ["diff", c.base, "-M", ...COLOR, "--", ...literal(part.flatMap((f) => (f.oldPath ? [f.oldPath, f.path] : [f.path])))], { raw: true }),
      (f) => tooBig(f.path),
    ),
    batched(untracked.slice(0, MAX_UNTRACKED_READ), (part) => untrackedPatch(c.root, part), (f) => tooBig(f.path)),
  ]);
  const patch = fitPatch(diff + added, { total: MAX_PATCH_BYTES });
  const skipped = untracked.length - Math.min(untracked.length, MAX_UNTRACKED_READ);
  return { patch, note: skipped ? ` · ${plural(skipped, "new file")} not shown` : "" };
}

/** Opens the viewer at once in its loading state and fills it when git answers. */
export function openChanges(root: string, mode: Mode, path: string | null) {
  const title = path ?? (mode === "branch" ? "Changes on this branch" : "Uncommitted changes");
  return openDiffWhile({ title, cwd: root, subtitle: `${basename(root)} · ${mode === "branch" ? "This branch" : "Working tree against HEAD"}` }, async () => {
    const c = await changes(root, mode);
    if (c.files.length === 0) return { error: mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes." };
    const { patch, note } = await patchOf(c, path);
    return patch ? { patch } : { error: `No diff to show${note}.` };
  });
}
