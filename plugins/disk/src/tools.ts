// The tools Claude gets. The build writes them into `contributes.mcpTools` of the manifest, so they run in Node then: nothing
// here, or in what it imports, may touch `wings` while it loads. They share main's scans and Collector, so what Claude
// scans shows in the Disk usage tab, and what it collects waits there for the user.

import { errorMessage, mapLimit } from "wings-plugin-sdk/async";
import { s, tool } from "wings-plugin-sdk/tools";

import { bytes } from "./lib/format";
import type { Collected, Entry, Hidden } from "./lib/messages";
import { protectedReason } from "./lib/protect";
import { childPath } from "./lib/tree";
import { collect, collectorNow } from "./main/collector";
import { disksNow, loadDisks } from "./main/disks";
import { entryAt, holders, resolve } from "./main/scans";

const size = (n: number) => ({ size: n, sizeText: bytes(n) });
const pathInput = s.string({ minLength: 1 }).describe("An absolute path to a folder or a file, or one that starts with ~ for the home folder. `/` or a disk's mount point is a whole disk.");

/** Up to this many entries go into one answer. */
const NODES = 300;

interface Tally {
  count?: number;
  items?: number;
  size: number;
  sizeText: string;
}

interface Node {
  name: string;
  kind: Entry["kind"];
  size: number;
  sizeText: string;
  items: number;
  restricted?: true;
  children?: Node[];
  others?: Tally;
  smaller?: Tally;
}

const nodeOf = (e: Entry): Node => ({ name: e.name, kind: e.kind, ...size(e.size), items: e.items, ...(e.restricted ? { restricted: true as const } : {}) });

/**
 * `root` as a node, with each folder's `limit` biggest entries under it, `depth` levels down. Folders open biggest first
 * until `NODES` entries are in, so a deep, wide tree can't make an answer too big to read: a folder not opened has no children.
 */
function listing(root: Entry, depth: number, limit: number): Node {
  const top = nodeOf(root);
  let budget = Math.max(NODES, limit);
  const queue = [{ entry: root, node: top, level: 1 }];
  while (queue.length) {
    queue.sort((a, b) => b.entry.size - a.entry.size);
    const { entry, node, level } = queue.shift()!;
    const kept = entry.children ?? [];
    const shown = kept.slice(0, Math.min(limit, budget));
    budget -= shown.length;
    if (shown.length) node.children = shown.map(nodeOf);
    if (kept.length > shown.length) node.others = { count: kept.length - shown.length, ...size(kept.slice(shown.length).reduce((sum, c) => sum + c.size, 0)) };
    if (entry.smaller) node.smaller = { items: entry.smaller.items, ...size(entry.smaller.size) };
    if (level >= depth) continue;
    for (const [i, child] of shown.entries()) if (child.kind === "dir") queue.push({ entry: child, node: node.children![i], level: level + 1 });
  }
  return top;
}

const hiddenView = (h: Hidden) => ({
  ...size(h.size),
  purgeable: size(h.purgeable),
  otherVolumes: h.others.map((v) => ({ name: v.name, mount: v.mount, ...size(v.used) })),
  snapshots: h.snapshots,
  rest: size(h.rest),
});

/** Every file the tree keeps at or under `entry`, with its full path. */
function filesUnder(entry: Entry, at: string, out: { path: string; size: number }[] = []) {
  if (entry.kind === "file") out.push({ path: at, size: entry.size });
  for (const child of entry.children ?? []) filesUnder(child, childPath(at, child.name), out);
  return out;
}

/**
 * Why `raw` can't be collected, before anything is spent on measuring it: measuring `/` or a home folder is a long walk
 * for a refusal. A `~` path is judged as under a home folder while no scan has told which one is the user's.
 */
function screen(raw: string, mounts: string[]): string | null {
  const full = resolve(raw);
  if (full !== null) return protectedReason(full, mounts);
  const stand = raw.replace(/^~(?=\/|$)/, "/Users/home");
  return protectedReason(stand, mounts)?.replace(stand, raw) ?? null;
}

/** What `raw` is: from a scan that holds it, else measured now, which also gives its full path. */
async function measure(raw: string): Promise<Collected> {
  const full = resolve(raw);
  const entry = full === null ? undefined : holders(full)[0]?.trail.at(-1);
  if (full !== null && entry) return { path: full, size: entry.size, kind: entry.kind };
  const { root } = await wings.diskUsage(full ?? raw, { depth: 0, maxEntries: 1 });
  return { path: root.name, size: root.size, kind: root.kind };
}

/** Whether `real`, the path Wings named, is the one `raw` reads as: a `~` path by what follows the `~`. macOS ignores case. */
function readsAs(raw: string, real: string): boolean {
  const full = resolve(raw);
  const wanted = (full ?? raw.replace(/^~/, "")).toLowerCase();
  return full !== null ? real.toLowerCase() === wanted : real.toLowerCase().endsWith(wanted);
}

export const tools = {
  list_disks: tool({
    description:
      "Lists the disks mounted on this Mac: name, mount point, kind, and total, used and free space (each in bytes and as text), the purgeable space macOS can free on its own, like local snapshots and caches, which Finder counts as free, and the other volumes that share the disk's space. Fast, and read-only.",
    async run() {
      return (await loadDisks()).map((d) => ({
        name: d.name,
        mount: d.mount,
        kind: d.kind,
        total: d.total,
        totalText: bytes(d.total),
        used: d.used,
        usedText: bytes(d.used),
        free: d.free,
        freeText: bytes(d.free),
        purgeable: d.purgeable,
        purgeableText: bytes(d.purgeable),
        others: d.others.map((v) => ({ name: v.name, mount: v.mount, used: v.used, usedText: bytes(v.used) })),
      }));
    },
  }),

  folder_usage: tool({
    description:
      "Shows what takes the space in a folder or on a whole disk, scanned in Wings, much faster than du: its size, and its biggest files and folders, `depth` levels down (1 to 4, 2 by default) with the `limit` biggest in each folder (1 to 50, 15 by default). Folders open biggest first until about 300 entries are in the answer, so a folder with no `children` was cut short, or is empty: ask about it by its path. In a folder, `others` is what `limit` left out and `smaller` is what the scan didn't keep one by one. `restricted` marks a folder Wings couldn't open (permissions, or macOS privacy) whose size isn't counted. For `/` or another disk's mount point it also gives `hidden`: the used space no folder explains, like purgeable space, other volumes and local snapshots. A scan under 30 minutes old that holds the path is reused, and a folder it only scanned coarsely is scanned again in more detail first; `fresh` scans again instead. A first scan of a whole disk takes up to a minute. A call is cut off after 120 seconds, and a scan still running then goes on and is used by the next call. The scan also shows in the Disk usage tab. Read-only.",
    input: s.object({
      path: pathInput,
      depth: s.integer({ minimum: 1, maximum: 4 }).describe("How many levels of folders to list. 2 by default.").optional(),
      limit: s.integer({ minimum: 1, maximum: 50 }).describe("The most entries to list in each folder, biggest first. 15 by default.").optional(),
      fresh: s.boolean().describe("Scan again even when a scan under 30 minutes old holds this path.").optional(),
    }),
    async run({ path, depth = 2, limit = 15, fresh = false }) {
      const { scan, entry, path: full } = await entryAt(path, { fresh });
      const top = listing(entry, depth, limit);
      return {
        path: full,
        kind: top.kind,
        size: top.size,
        sizeText: top.sizeText,
        items: top.items,
        scannedAt: new Date(scan.at).toISOString(),
        children: top.children,
        others: top.others,
        smaller: top.smaller,
        hidden: entry === scan.tree && scan.hidden ? hiddenView(scan.hidden) : undefined,
        notes: scan.restricted > 0 ? [`${scan.restricted} folders couldn't be opened, so their sizes aren't counted. Full Disk Access for Wings in System Settings shows them.`] : undefined,
      };
    },
  }),

  largest_files: tool({
    description:
      "Lists the biggest files under a folder or on a whole disk, biggest first, with their full paths and sizes. It uses the same scan as folder_usage (one under 30 minutes old that holds the path is reused, a coarse folder is scanned in more detail first, and a first scan of a whole disk takes up to a minute), so it lists only the files the scan kept: the answer says the size under which files aren't listed one by one. `limit` is 1 to 100, 25 by default. Read-only.",
    input: s.object({ path: pathInput, limit: s.integer({ minimum: 1, maximum: 100 }).describe("How many files to list. 25 by default.").optional() }),
    async run({ path, limit = 25 }) {
      const { scan, entry, path: full, detail } = await entryAt(path);
      return {
        path: full,
        scannedAt: new Date(scan.at).toISOString(),
        files: filesUnder(entry, full)
          .sort((a, b) => b.size - a.size)
          .slice(0, limit)
          .map((f) => ({ path: f.path, ...size(f.size) })),
        note: `Files under ${bytes(detail)} aren't listed: the scan folded them into their folders.`,
      };
    },
  }),

  collect: tool({
    description:
      "Puts files and folders in the Collector of the Disk usage tab, the list the user reviews and moves to the Trash from there. This never moves anything to the Trash. A folder takes out what is collected inside it, and a path already collected or inside a collected folder is left as it is. Disks, system folders, home folders and their standard folders (Library, Desktop, Documents, Downloads, Applications, Movies, Music, Pictures, Public) are refused, and each refusal says why; what is inside them may be collected. Sizes come from a scan that holds the path, else are measured now, which takes a while for a big folder. Up to 200 paths, each absolute or starting with ~.",
    input: s.object({ paths: s.array(s.string({ minLength: 1 }), { maxItems: 200 }).describe("The files and folders to collect.") }),
    async run({ paths }) {
      if (!paths.length) throw new Error("paths needs at least 1 path.");
      const mounts = (await disksNow()).rows.map((d) => d.mount);
      const refused: { path: string; reason: string }[] = [];
      const measured = await mapLimit(paths, 4, async (raw): Promise<Collected | null> => {
        const reason = screen(raw, mounts);
        if (reason) {
          refused.push({ path: raw, reason });
          return null;
        }
        try {
          const item = await measure(raw);
          // Wings names the real path, with every link on the way followed, and the Trash would act on that one.
          if (!readsAs(raw, item.path)) {
            refused.push({ path: raw, reason: `${raw} is a link or goes through one. Its real path is ${item.path}: collect that instead.` });
            return null;
          }
          return item;
        } catch (error) {
          refused.push({ path: raw, reason: `${raw} couldn't be measured: ${errorMessage(error)}` });
          return null;
        }
      });
      const result = await collect(measured.filter((item) => item !== null));
      refused.push(...result.refused);
      const { items, total } = await collectorNow();
      return {
        added: result.added.map((item) => ({ path: item.path, sizeText: bytes(item.size) })),
        refused,
        count: items.length,
        total,
        totalText: bytes(total),
        next: "The user reviews these in the Disk usage tab's Collector and moves them to the Trash from there.",
      };
    },
  }),
};
