import { plural } from "./format";
import { changes, MAX_UNTRACKED_READ, type Changes, type FileChange } from "./changes";
import { basename, COLOR, git, literal } from "./git";
import type { Mode } from "./types";

/** About 2 MB of patch. Past this the viewer would take seconds to draw, so "View all" says so instead. */
export const MAX_PATCH_LINES = 50_000;
export const MAX_PATCH_BYTES = 2 * 1024 * 1024;
export const MAX_PATCH_FILES = 1_500;

/** A patch for files git doesn't track yet, read with `git grep`, since Wings refuses `git diff --no-index`. */
async function untrackedPatch(root: string, files: FileChange[]) {
  const read = files.slice(0, MAX_UNTRACKED_READ);
  if (read.length === 0) return "";
  const paths = read.map((f) => f.path);
  const r = await wings.exec("git", ["grep", "--untracked", "-I", "-z", "--no-color", "--no-line-number", "--no-column", "-e", "", "--", ...literal(paths)], { cwd: root });
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
    const all = await wings.exec("git", ["grep", "--untracked", "-z", "--count", "--no-color", "-e", "", "--", ...literal(rest)], { cwd: root });
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
  const spec = path == null ? [] : ["--", ...literal(tracked.flatMap((f) => (f.oldPath ? [f.oldPath, f.path] : [f.path])))];
  const [diff, added] = await Promise.all([tracked.length ? git(c.root, ["diff", c.base, "-M", ...COLOR, ...spec]) : "", untrackedPatch(c.root, untracked)]);
  const patch = diff + added;
  if (patch.length > MAX_PATCH_BYTES) throw new Error(`Too large to show: ${Math.round(patch.length / 1024 / 1024)} MB of changes in ${plural(files.length, "file")}.`);
  const skipped = untracked.length - Math.min(untracked.length, MAX_UNTRACKED_READ);
  return { patch, note: skipped ? ` · ${plural(skipped, "new file")} not shown` : "" };
}

export const against = (mode: Mode, label: string) => (mode === "branch" ? `Since ${label}` : "Working tree against HEAD");

/** Opens the viewer at once in its loading state and fills it when git answers. */
export async function openChanges(root: string, mode: Mode, path: string | null) {
  const title = path ?? (mode === "branch" ? "Changes on this branch" : "Uncommitted changes");
  const { id } = await wings.openDiff({ title, subtitle: `${basename(root)} · ${mode === "branch" ? "This branch" : "Working tree against HEAD"}` });
  try {
    const c = await changes(root, mode);
    if (c.files.length === 0) return void (await wings.updateDiff(id, { error: mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes." }));
    const { patch, note } = await patchOf(c, path);
    if (!patch) return void (await wings.updateDiff(id, { error: `No diff to show${note}.` }));
    await wings.updateDiff(id, { patch });
  } catch (error) {
    await wings.updateDiff(id, { error: String((error as Error).message ?? error) });
  }
}
