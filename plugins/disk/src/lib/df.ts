// The disks overview's numbers: `df -k -P` read for what macOS can purge and which volumes share a disk's space, and the
// hidden space of a scanned disk.

import { basename } from "wings-plugin-sdk/format";

import type { DiskRow, Hidden, Volume } from "./messages";

/** A row of `df -k -P`, in bytes. */
export interface DfRow {
  device: string;
  total: number;
  used: number;
  available: number;
  mount: string;
}

// The device and the mount point can hold spaces (`map auto_home`, `devices -- file:///...`, a volume named `My Disk`), so a
// row is matched around its four number columns. The header has none, so it never matches.
const ROW = /^(.+?)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+%|-)\s+(.+)$/;

/** The rows of `df -k -P`. */
export function parseDf(text: string): DfRow[] {
  const rows: DfRow[] = [];
  for (const line of text.split("\n")) {
    const m = ROW.exec(line.trimEnd());
    if (m) rows.push({ device: m[1], total: +m[2] * 1024, used: +m[3] * 1024, available: +m[4] * 1024, mount: m[6] });
  }
  return rows;
}

/** The free space df reports for the volume mounted at `mount`, which leaves out what macOS can purge, or null when df has no row for it. */
export const availableOf = (df: DfRow[], mount: string): number | null => df.find((r) => r.mount === mount)?.available ?? null;

/** The APFS container a device belongs to: `disk3` for `/dev/disk3s5`. */
const containerOf = (device: string) => /^\/dev\/(disk\d+)/.exec(device)?.[1];

/** The disks `wings.disks()` lists, with what df adds: the purgeable space, and the other volumes in the same container. */
export function diskRows(disks: WingsDisk[], df: DfRow[]): DiskRow[] {
  // A volume's name is its mount's last part, which the startup disk's `/` has none of.
  const nameOf = (mount: string) => (mount === "/" ? (disks.find((d) => d.mount === "/")?.name ?? "/") : basename(mount));
  return disks.map((disk) => {
    const own = df.find((r) => r.mount === disk.mount);
    // Only the volumes of an APFS container share space: two partitions of an exFAT disk don't.
    const container = own && disk.fs === "apfs" ? containerOf(own.device) : undefined;
    const others: Volume[] = [];
    for (const row of df) {
      if (!container || containerOf(row.device) !== container || row.mount === disk.mount || row.used <= 0) continue;
      // A scan of `/` reaches the Data volume through its firmlinks.
      if (disk.mount === "/" && row.mount === "/System/Volumes/Data") continue;
      others.push({ name: nameOf(row.mount), mount: row.mount, used: row.used });
    }
    others.sort((a, b) => b.used - a.used);
    return { ...disk, used: Math.max(0, disk.total - disk.free), purgeable: own ? Math.max(0, disk.free - own.available) : 0, others };
  });
}

/**
 * The used space a scan of `disk` didn't find, given the bytes it did (`found`) and the disk's local snapshot dates.
 * `available` is df's free space for it, which leaves out what macOS can purge: the used space counts that as used.
 */
export function hiddenSpace(disk: DiskRow, available: number | null, found: number, snapshots: string[]): Hidden {
  const usedAll = disk.total - (available ?? disk.free);
  const size = Math.max(0, usedAll - found);
  const rest = Math.max(0, size - disk.purgeable - disk.others.reduce((sum, v) => sum + v.used, 0));
  return { size, purgeable: disk.purgeable, others: disk.others, snapshots, rest };
}
