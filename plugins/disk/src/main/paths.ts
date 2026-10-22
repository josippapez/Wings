// Paths the way main and Claude's tools meet them: with trailing slashes, or starting with `~`.

import type { Scan } from "../lib/messages";

/** `path` without trailing slashes: `/Users/me/` is `/Users/me`, and `/` stays. */
export const tidy = (path: string) => (path.length > 1 ? path.replace(/\/+$/, "") || "/" : path);

/** Whether `path` is below `folder`, and not the folder itself. */
export const inside = (path: string, folder: string) => path.startsWith(folder.endsWith("/") ? folder : `${folder}/`) && path.length > folder.length;

/**
 * The home folder, once a scan of `~` or of a folder under it is done: Wings turned `~` into the tree's name there, so the
 * name less what followed the `~` is the folder `~` stands for. Null before that: nothing else in a plugin says where it is.
 */
export function homeFolder(scans: Iterable<Scan>): string | null {
  for (const scan of scans) {
    if (scan.state !== "done" || (scan.root !== "~" && !scan.root.startsWith("~/"))) continue;
    const after = tidy(scan.root).slice(1);
    if (scan.tree.name.endsWith(after)) return scan.tree.name.slice(0, scan.tree.name.length - after.length) || "/";
  }
  return null;
}

/** `path` with its leading `~` made the home folder, tidied; null for a `~` path while the home folder isn't known. */
export function expandHome(path: string, home: string | null): string | null {
  const full = tidy(path);
  if (full !== "~" && !full.startsWith("~/")) return full;
  return home === null ? null : tidy(home + full.slice(1));
}
