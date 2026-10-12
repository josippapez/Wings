// ---------- diffs: an edited file, or everything changed, against HEAD ----------

import { basename } from "./format";
import { sessions } from "./session";

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

/** Opens the viewer at once and fills it in when the command answers, so a click never waits on it. `path` null is the whole working tree. */
export async function openChanges(sessionId: string, path: string | null) {
  const s = sessions.get(sessionId);
  if (!s?.cwd) return;
  const file = path == null ? null : s.files.has(path) ? inside(s.cwd, path) : null;
  if (path != null && !file) return;
  const title = file ? basename(file) : "All changes";
  const { id } = await wings.openDiff({ title, cwd: s.cwd, subtitle: file ? "Working tree against HEAD" : `Working tree against HEAD · ${basename(s.cwd)}` });
  try {
    const args = file ? ["diff", "HEAD", "--no-color", "--no-ext-diff", "--", file] : ["diff", "HEAD", "--no-color", "--no-ext-diff"];
    const r = await wings.exec("git", args, { cwd: s.cwd });
    if (r.code !== 0) throw new Error(r.stderr.trim() || "git diff failed");
    if (r.stdout.trim()) return void (await wings.updateDiff(id, { patch: r.stdout }));
    // Nothing against HEAD: a file that isn't tracked yet has no diff to show, since Wings doesn't allow `--no-index`.
    const tracked = file ? (await wings.exec("git", ["ls-files", "--error-unmatch", "--", file], { cwd: s.cwd })).code === 0 : true;
    await wings.updateDiff(id, { error: tracked ? "No changes against HEAD." : "This file isn't tracked yet, so there is no diff." });
  } catch (error) {
    await wings.updateDiff(id, { error: String((error as Error).message ?? error) });
  }
}
