// ---------- diffs: an edited file, or everything changed, against HEAD ----------

import { openDiffWhile } from "wings-plugin-sdk/async";
import { basename } from "wings-plugin-sdk/format";

import type { Session } from "./session";

/** The normalised absolute path when it's inside `cwd`, else null. Paths come from the transcript, so `..` can't lead out. */
function inside(cwd: unknown, path: unknown) {
  if (typeof cwd !== "string" || typeof path !== "string" || !cwd.startsWith("/") || !path.startsWith("/")) return null;
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  const root = cwd.replace(/\/+$/, "");
  const full = `/${parts.join("/")}`;
  return root && full.startsWith(`${root}/`) ? full : null;
}

/**
 * Opens the viewer at once and fills it in when the command answers, so a click never waits on it. `path` null is the
 * whole working tree. A request that can't be shown throws why, for the sidebar to say.
 */
export function openChanges(s: Session | undefined, path: string | null) {
  if (!s?.cwd) throw new Error("This session isn't in a pane any more.");
  const cwd = s.cwd;
  if (path != null && !s.files.has(path)) throw new Error("This session hasn't edited that file in this run.");
  const file = path == null ? null : inside(cwd, path);
  if (path != null && !file) throw new Error(`${basename(path)} is outside ${basename(cwd)}, so there is no diff to show.`);
  return openDiffWhile({ title: file ? basename(file) : "All changes", cwd, subtitle: file ? "Working tree against HEAD" : `Working tree against HEAD · ${basename(cwd)}` }, async () => {
    const args = file ? ["diff", "HEAD", "--no-color", "--no-ext-diff", "--", file] : ["diff", "HEAD", "--no-color", "--no-ext-diff"];
    const r = await wings.exec("git", args, { cwd });
    if (r.code !== 0) throw new Error(r.stderr.trim() || "git diff failed");
    if (r.stdout.trim()) return { patch: r.stdout };
    // Nothing against HEAD: a file that isn't tracked yet has no diff to show, since Wings doesn't allow `--no-index`.
    const tracked = file ? (await wings.exec("git", ["ls-files", "--error-unmatch", "--", file], { cwd })).code === 0 : true;
    return { error: tracked ? "No changes against HEAD." : "This file isn't tracked yet, so there is no diff." };
  });
}
