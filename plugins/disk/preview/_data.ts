// Made-up disks, scans and Collector for the fixtures, on the startup disk's tree in _tree.ts.

import type { Collected, Collector, DiskRow, Entry, Hidden, ScanSummary, TrashResult } from "../src/lib/messages";
import type { DoneScan, RunningScan } from "../src/ui/model";
import { find } from "../src/lib/tree";
import { homePath, root, tree } from "./_tree";

export { homePath, root, tree };

const GB = 1e9;
const TB = 1e12;
const now = Date.now();

const disk = (d: Omit<DiskRow, "used" | "readOnly" | "fs"> & Partial<Pick<DiskRow, "fs" | "readOnly">>): DiskRow => ({ fs: "apfs", readOnly: false, ...d, used: d.total - d.free });

export const disks: DiskRow[] = [
  disk({
    name: "Macintosh HD",
    mount: "/",
    kind: "ssd",
    removable: false,
    total: 994.7 * GB,
    free: 312.4 * GB,
    purgeable: 41.2 * GB,
    others: [
      { name: "VM", mount: "/System/Volumes/VM", used: 21.5 * GB },
      { name: "Preboot", mount: "/System/Volumes/Preboot", used: 6.1 * GB },
      { name: "Update", mount: "/System/Volumes/Update", used: 7.2 * GB },
    ],
  }),
  disk({ name: "Backup", mount: "/Volumes/Backup", fs: "hfs", kind: "hdd", removable: true, total: 2 * TB, free: 1.38 * TB, purgeable: 0, others: [] }),
  disk({ name: "Photos", mount: "/Volumes/Photos", fs: "exfat", kind: "ssd", removable: true, total: 500 * GB, free: 38 * GB, purgeable: 0, others: [] }),
  disk({ name: "Spare", mount: "/Volumes/Spare", kind: "unknown", removable: false, total: 256 * GB, free: 70 * GB, purgeable: 0, others: [] }),
];

export const hidden: Hidden = {
  size: 96.4 * GB,
  purgeable: 41.2 * GB,
  others: disks[0].others,
  snapshots: ["2026-10-22-101010", "2026-10-21-181533", "2026-10-21-091205"],
  rest: 20.4 * GB,
};

export const done: DoneScan = {
  state: "done",
  root,
  tree,
  floor: 1_048_576,
  entries: 1_204_331,
  restricted: 4,
  ms: 83_000,
  at: now - 5 * 60_000,
  disk: "/",
  hidden,
  refining: [],
};

/** A scan of the home folder: a folder's tree has no disk, so no hidden space. */
const home = find(tree, homePath)?.at(-1);
export const homeScan: DoneScan = { ...done, root: "~", tree: { ...(home as Entry), name: homePath }, disk: null, hidden: null };

export const scanning: RunningScan = {
  state: "scanning",
  root,
  progress: { entries: 612_408, bytes: 391.4 * GB, path: "/Users/ada/Library/Developer/Xcode/DerivedData/Wings-bqzkdyutvmgwasfjzvodpbkiqesn/Build/Intermediates.noindex" },
  expected: disks[0].used,
  startedAt: now - 83_000,
};

/** A folder with no earlier scan to say how big it is. */
export const scanningFolder: RunningScan = {
  state: "scanning",
  root: "~/Code",
  progress: { entries: 48_220, bytes: 11.7 * GB, path: "/Users/ada/Code/project-c/node_modules/.pnpm/typescript@6.0.3/node_modules/typescript/lib" },
  expected: null,
  startedAt: now - 9_000,
};

export const scans: ScanSummary[] = [
  { root, state: "done", size: tree.size, at: now - 5 * 60_000 },
  { root: "/Users/ada/Code", state: "done", size: 128.4 * GB, at: now - 2 * 3_600_000 },
  { root: "/Volumes/Backup", state: "scanning", size: null, at: now - 20_000 },
  { root: "~/Downloads", state: "error", size: null, at: now - 30 * 3_600_000 },
];

export const projects: WingsProject[] = [
  { id: "p1", name: "web", path: "/work/acme/web", branch: "main" },
  { id: "p2", name: "api", path: "/work/acme/api", branch: "fix/login" },
];

const picks: Collected[] = [
  { path: "/Users/ada/Library/Developer/Xcode/DerivedData", size: 38 * GB, kind: "dir" },
  { path: "/Users/ada/Downloads/Xcode_27.xip", size: 11.2 * GB, kind: "file" },
  { path: "/Users/ada/Movies/Screen Recording.mov", size: 4.9 * GB, kind: "file" },
];
export const collector: Collector = { items: picks, total: picks.reduce((sum, p) => sum + p.size, 0) };

/** The small files a folder folded into its smaller items, as `listAll` finds them. */
const small = Array.from({ length: 18 }, (_, i): Entry => ({ name: `Scan-${String(i + 1).padStart(3, "0")}.pdf`, size: 900_000 - i * 31_000, kind: "file", items: 0 }));

/** What main answers `listAll` with for a folder of the tree: what it keeps, and the small files besides. */
export function listAll(path: string): Entry {
  const entry = find(tree, path)?.at(-1);
  if (!entry) throw new Error("That folder isn't in the scan.");
  return { name: entry.name, size: entry.size, kind: "dir", items: entry.items, children: [...(entry.children ?? []), ...small] };
}

export const trashed: TrashResult = {
  trashed: [picks[0].path, picks[1].path],
  failed: [{ path: picks[2].path, message: "Operation not permitted" }],
  freed: picks[0].size + picks[1].size,
};
