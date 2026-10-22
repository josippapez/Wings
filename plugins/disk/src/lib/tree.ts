// Paths in a scan's tree. The root entry's name is its full path, and every other entry's is its own name.

import type { Entry } from "./messages";

/** The path of `name` in the folder at `parent`. */
export const childPath = (parent: string, name: string) => (parent.endsWith("/") ? parent + name : `${parent}/${name}`);

/** The folder `path` is in, or null for `/`. */
export function parentPath(path: string): string | null {
  if (path === "/") return null;
  const at = path.lastIndexOf("/");
  return at <= 0 ? "/" : path.slice(0, at);
}

/** `path`'s names below `root`, like `["Library", "Caches"]`: `[]` for the root itself, null for a path outside it. */
export function partsBelow(root: string, path: string): string[] | null {
  if (path === root) return [];
  const prefix = root.endsWith("/") ? root : `${root}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length).split("/") : null;
}

/**
 * The entry at `path` in `tree`, with the entries from the root down to it (the root first, the entry last), or null
 * when the tree doesn't hold it, like a file folded into its folder's smaller entries.
 */
export function find(tree: Entry, path: string): Entry[] | null {
  const parts = partsBelow(tree.name, path);
  if (!parts) return null;
  const trail = [tree];
  for (const name of parts) {
    const next = trail[trail.length - 1].children?.find((c) => c.name === name);
    if (!next) return null;
    trail.push(next);
  }
  return trail;
}
