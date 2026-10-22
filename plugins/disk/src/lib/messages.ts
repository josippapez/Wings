// What main and the Disk usage tab say to each other. Main runs every scan and keeps the results, so each tab and
// Claude's tools see the same ones, and a tab asks main for anything that touches the disk.

import { channel } from "wings-plugin-sdk/messages";

/** A scanned file or folder. A scan's tree is its root entry, whose `name` is the full path. */
export interface Entry {
  name: string;
  size: number;
  kind: WingsDiskEntry["kind"];
  items: number;
  children?: Entry[];
  smaller?: { size: number; items: number };
  restricted?: true;
  /** The floor a later, finer scan of this folder used, when main refined it. Its entries are kept down to this size. */
  detail?: number;
}

/** A disk as the overview shows it. */
export interface DiskRow extends WingsDisk {
  /** `total - free`: what Finder calls used. */
  used: number;
  /** What macOS can free on its own, like local snapshots and caches. Finder counts it as free. 0 when unknown. */
  purgeable: number;
  /** Other volumes that share this disk's space, like VM and Preboot on an APFS startup disk. */
  others: Volume[];
}

export interface Volume {
  name: string;
  mount: string;
  used: number;
}

/** The used space a scan of a disk didn't find, like DaisyDisk's "hidden space". */
export interface Hidden {
  size: number;
  purgeable: number;
  others: Volume[];
  /** Dates of Time Machine local snapshots, like `2026-10-22-101010`. Their space is part of `purgeable`. */
  snapshots: string[];
  /** The rest: folders Wings can't open, other users' files, system data. */
  rest: number;
}

export type Scan =
  | {
      state: "scanning";
      root: string;
      progress: WingsDiskScanProgress;
      /** The bytes it should find, when known: the disk's used space, or the folder's size from an earlier scan. */
      expected: number | null;
      startedAt: number;
    }
  | {
      state: "done";
      root: string;
      tree: Entry;
      floor: number;
      entries: number;
      restricted: number;
      ms: number;
      at: number;
      /** The mount point when `root` is a whole disk. */
      disk: string | null;
      hidden: Hidden | null;
      /** Folders main is scanning in more detail right now. */
      refining: string[];
    }
  | { state: "error"; root: string; message: string };

export interface ScanSummary {
  root: string;
  state: Scan["state"];
  /** The tree's size once done. */
  size: number | null;
  at: number;
}

export interface Collected {
  path: string;
  size: number;
  kind: Entry["kind"];
}

export interface Collector {
  items: Collected[];
  total: number;
}

export interface TrashResult {
  trashed: string[];
  failed: { path: string; message: string }[];
  /** The bytes of what moved. */
  freed: number;
}

export const ch = channel<
  {
    /** Scans `root`, or scans it again. Progress and the result come through the `scan` query. */
    scan: { request: { root: string }; response: void };
    stop: { request: { root: string }; response: void };
    /** Drops a scan's results, to free the memory they hold. */
    forget: { request: { root: string }; response: void };
    /** Scans a folder of a done scan again in more detail and puts it into the tree. */
    refine: { request: { root: string; path: string }; response: void };
    /** Every entry directly in a folder, small ones included: what "smaller objects" stands for. */
    listAll: { request: { path: string }; response: Entry };
    /** Rejects with the reason when a path may not be collected, like a system folder. */
    collect: { request: { items: Collected[] }; response: void };
    uncollect: { request: { paths: string[] }; response: void };
    /** Moves everything in the Collector to the Trash and takes it out of every scan. */
    trash: { request: void; response: TrashResult };
    reveal: { request: { path: string }; response: void };
    preview: { request: { path: string }; response: void };
    deleteSnapshot: { request: { date: string }; response: void };
    /** Asks macOS to free the purgeable space of a disk by thinning its local snapshots. */
    purge: { request: { mount: string }; response: void };
  },
  {},
  {
    disks: { key: void; value: DiskRow[] };
    scans: { key: void; value: ScanSummary[] };
    scan: { key: { root: string }; value: Scan | null };
    collector: { key: void; value: Collector };
  }
>();
