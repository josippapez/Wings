// The rows of the list beside the map, in the order the arrow keys walk them.

import type { Collected, Entry } from "../lib/messages";
import { childPath } from "../lib/tree";

/** An entry of the tree, one of a folder's smaller items that `listAll` found, the row that stands for those, or the hidden space. */
export type RowKind = "entry" | "extra" | "smaller" | "hidden";

export interface Row {
  key: string;
  kind: RowKind;
  /** An entry's path, or the folder a `smaller` row is in. Empty for the hidden space. */
  path: string;
  name: string;
  size: number;
  /** What a folder holds, or what the smaller items add up to. */
  items: number;
  entryKind: Entry["kind"] | null;
  /** A folder the map can go into. */
  openable: boolean;
  /** A folder Wings couldn't open. */
  restricted: boolean;
}

export const HIDDEN_KEY = "hidden";
export const smallerKey = (folder: string) => `smaller:${folder}`;
export const entryKey = (path: string) => `entry:${path}`;

/** What `listAll` is doing for a folder: one's smaller items, every entry in it. */
export type Extras = { state: "loading" } | { state: "ready"; entries: Entry[] } | { state: "error"; message: string };

function rowOf(kind: "entry" | "extra", folder: string, entry: Entry): Row {
  const path = childPath(folder, entry.name);
  const restricted = entry.restricted === true;
  return {
    key: kind === "entry" ? entryKey(path) : `extra:${path}`,
    kind,
    path,
    name: entry.name,
    size: entry.size,
    items: entry.items,
    entryKind: entry.kind,
    openable: kind === "entry" && entry.kind === "dir" && !restricted,
    restricted,
  };
}

/**
 * The rows for the folder at `folder`: its entries, a row for what it folded into smaller items, the ones `listAll` found
 * under that row once it is open (the ones the folder doesn't already list, biggest first), and the hidden space.
 */
export function rowsOf(folder: string, entry: Entry, options: { smallerOpen: boolean; extras: Extras | undefined; hidden: number | null }): Row[] {
  const rows = (entry.children ?? []).map((child) => rowOf("entry", folder, child));
  if (entry.smaller) {
    rows.push({ key: smallerKey(folder), kind: "smaller", path: folder, name: "smaller items", size: entry.smaller.size, items: entry.smaller.items, entryKind: null, openable: false, restricted: false });
    if (options.smallerOpen && options.extras?.state === "ready") {
      const listed = new Set((entry.children ?? []).map((c) => c.name));
      const more = options.extras.entries.filter((e) => !listed.has(e.name)).sort((a, b) => b.size - a.size);
      rows.push(...more.map((e) => rowOf("extra", folder, e)));
    }
  }
  if (options.hidden !== null && options.hidden > 0) {
    rows.push({ key: HIDDEN_KEY, kind: "hidden", path: "", name: "Hidden space", size: options.hidden, items: 0, entryKind: null, openable: false, restricted: false });
  }
  return rows;
}

/** What goes in the Collector for a row, or null for a row that can't: a folder Wings couldn't open, the smaller items, the hidden space. */
export function collectedOf(row: Row): Collected | null {
  return (row.kind === "entry" || row.kind === "extra") && !row.restricted && row.entryKind ? { path: row.path, size: row.size, kind: row.entryKind } : null;
}
