// Edits to a scan's tree, which main keeps: a finer scan of one folder put in, and what moved to the Trash taken out.
// Both leave the sizes and item counts of every folder above right, and every folder's children biggest first.

import type { Entry } from "./messages";
import { partsBelow } from "./tree";

const bySize = (a: Entry, b: Entry) => b.size - a.size;

/** The entries from `tree` down along `path` as far as the tree holds it, and whether it holds all of it. Null for a path outside the tree. */
function walk(tree: Entry, path: string): { trail: Entry[]; whole: boolean } | null {
  const parts = partsBelow(tree.name, path);
  if (!parts) return null;
  const trail = [tree];
  for (const name of parts) {
    const next = trail[trail.length - 1].children?.find((c) => c.name === name);
    if (!next) return { trail, whole: false };
    trail.push(next);
  }
  return { trail, whole: true };
}

/** A folder whose size changed may no longer sit where its parent lists it. */
function resort(folders: Entry[]) {
  for (const folder of folders) folder.children?.sort(bySize);
}

/**
 * Puts `fresh`, a scan of the folder at `path`, into `tree`: the folder takes its children, smaller objects, size and
 * items, remembers the `detail` that scan kept (its floor), and every folder above it changes by the difference in size and
 * items. False when the tree holds no folder at `path`, and then nothing changed.
 */
export function graft(tree: Entry, path: string, fresh: Entry, detail: number): boolean {
  const found = walk(tree, path);
  if (!found?.whole) return false;
  const entry = found.trail[found.trail.length - 1];
  if (entry.kind !== "dir") return false;
  const above = found.trail.slice(0, -1);
  const size = fresh.size - entry.size;
  const items = fresh.items - entry.items;
  entry.size = fresh.size;
  entry.items = fresh.items;
  if (fresh.children) entry.children = fresh.children;
  else delete entry.children;
  if (fresh.smaller) entry.smaller = fresh.smaller;
  else delete entry.smaller;
  entry.detail = detail;
  for (const folder of above) {
    folder.size += size;
    folder.items += items;
  }
  resort(above);
  return true;
}

/** What `removePath` did: took the entry out, took its bytes out of a folder's smaller objects, or found the tree's own root. */
export type Removal = "entry" | "folded" | "root";

/**
 * Takes the file or folder at `path` out of `tree` after it moved to the Trash, and returns what it did, or null when the
 * tree has nothing there. An entry the tree keeps is removed from its parent, and its size and items (+ 1 for itself) come
 * off every folder above. One the scan folded into a folder's smaller objects isn't in the tree: its `size` comes off the
 * deepest folder that holds it, from its smaller objects as far as they go, and off every folder above. The tree's own root
 * is not removed: the caller drops the whole scan.
 */
export function removePath(tree: Entry, path: string, size: number): Removal | null {
  const found = walk(tree, path);
  if (!found) return null;
  const { trail, whole } = found;
  if (whole) {
    if (trail.length === 1) return "root";
    const entry = trail[trail.length - 1];
    const above = trail.slice(0, -1);
    const parent = above[above.length - 1];
    parent.children = parent.children!.filter((c) => c !== entry);
    for (const folder of above) {
      folder.size = Math.max(0, folder.size - entry.size);
      folder.items = Math.max(0, folder.items - entry.items - 1);
    }
    resort(above);
    return "entry";
  }
  const deepest = trail[trail.length - 1];
  if (deepest.kind !== "dir" || !deepest.smaller) return null;
  deepest.smaller.size -= Math.min(size, deepest.smaller.size);
  deepest.smaller.items = Math.max(0, deepest.smaller.items - 1);
  if (deepest.smaller.size <= 0 && deepest.smaller.items <= 0) delete deepest.smaller;
  for (const folder of trail) {
    folder.size = Math.max(0, folder.size - size);
    folder.items = Math.max(0, folder.items - 1);
  }
  resort(trail);
  return "folded";
}
