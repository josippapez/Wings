// What the tab works out from a scan, with no React in it: names, paths and the sentences it writes.

import { basename } from "wings-plugin-sdk/format";
import { bytes } from "../lib/format";
import type { Collected, DiskRow, Entry, Scan, TrashResult } from "../lib/messages";
import { childPath, find, parentPath } from "../lib/tree";
import type { Message } from "./parts";

export type DoneScan = Extract<Scan, { state: "done" }>;
export type RunningScan = Extract<Scan, { state: "scanning" }>;

/** What a scan of `root` is called: the disk's name for a whole disk, the home folder, or the folder's own name. */
export function rootName(root: string, disks: DiskRow[] | null): string {
  const disk = disks?.find((d) => d.mount === root);
  if (disk) return disk.name;
  return root === "~" ? "Home folder" : basename(root);
}

/** `n` and its noun with the thousands told apart: "1 item", "90,000 items". */
export const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** `ms` as `m:ss`, or `h:mm:ss` from an hour on. */
export function duration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const s = String(total % 60).padStart(2, "0");
  const m = Math.floor(total / 60);
  return m < 60 ? `${m}:${s}` : `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}:${s}`;
}

/** A Time Machine snapshot's date, `2026-10-22-101010`, as the user's locale writes it. What isn't one stays as it is. */
export function snapshotLabel(date: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})-(\d{2})(\d{2})(\d{2})$/.exec(date);
  if (!m) return date;
  const [year, month, day, hour, minute, second] = m.slice(1).map(Number);
  return new Date(year, month - 1, day, hour, minute, second).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** The folder the tab asks for, or the nearest one above it that the tree holds: the tree's root when none is. */
export function existingFocus(tree: Entry, wanted: string | null): string {
  for (let path = wanted ?? tree.name; path; path = parentPath(path) ?? "") {
    const entry = find(tree, path)?.at(-1);
    if (entry && entry.kind === "dir" && !entry.restricted) return path;
  }
  return tree.name;
}

/** The least size main keeps when it scans a folder of `size` bytes alone. The same rule as `detailFor` in src/main/scans.ts. */
const detailFor = (size: number) => Math.max(4096, Math.round(size / 40_000));

/**
 * Whether the folder at the end of `trail`, the entries from the scan's root down to it, was kept down to a floor that hides
 * most of what is in it, and a scan of it alone would keep a finer one. The floor that applies is the `detail` of the nearest
 * folder on the trail that was scanned again, itself included, else the scan's. The same rule as `coarse` in src/main/scans.ts,
 * so the page asks main to refine a folder only when main would.
 */
export function isCoarse(scan: DoneScan, trail: Entry[]): boolean {
  const entry = trail[trail.length - 1];
  const floor = trail.findLast((e) => e.detail !== undefined)?.detail ?? scan.floor;
  return entry.kind === "dir" && !entry.restricted && entry.detail === undefined && floor * 2000 > entry.size && detailFor(entry.size) < floor;
}

export interface Crumb {
  name: string;
  path: string;
}

/** The folders from the tree's root down to `focus`, the root called `rootLabel`. */
export function crumbsOf(tree: Entry, focus: string, rootLabel: string): Crumb[] {
  const trail = find(tree, focus) ?? [tree];
  let path = tree.name;
  return trail.map((entry, i) => {
    if (i > 0) path = childPath(path, entry.name);
    return { name: i === 0 ? rootLabel : entry.name, path };
  });
}

/** What a dragged wedge or row carries, as the Collector takes it: the items in it, or none when it isn't ours. */
export function parseDrop(text: string): Collected[] {
  try {
    const raw: unknown = JSON.parse(text);
    return (Array.isArray(raw) ? raw : [raw]).filter(
      (c): c is Collected => !!c && typeof c.path === "string" && c.path.startsWith("/") && Number.isFinite(c.size) && ["dir", "file", "link", "other"].includes(c.kind),
    );
  } catch {
    return [];
  }
}

/** What to tell about a move to the Trash: what moved, and each thing that didn't with its reason. */
export function trashMessages(result: TrashResult): Omit<Message, "id">[] {
  const out: Omit<Message, "id">[] = [];
  if (result.trashed.length) out.push({ tone: "success", text: `Moved ${count(result.trashed.length, "item")}, ${bytes(result.freed)}, to the Trash. Empty the Trash to get the space back.` });
  if (result.failed.length) out.push({ tone: "danger", text: `${count(result.failed.length, "item")} didn't move:\n${result.failed.map((f) => `${basename(f.path)}: ${f.message}`).join("\n")}` });
  return out;
}
