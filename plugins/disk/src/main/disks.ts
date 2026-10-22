// The disks: what Wings lists for each one, `df`'s numbers on top, and the Time Machine snapshots of a disk.

import { availableOf, diskRows, hiddenSpace, parseDf, type DfRow } from "../lib/df";
import type { DiskRow, Hidden } from "../lib/messages";

/** A local Time Machine snapshot's date, as `tmutil` writes it. */
export const SNAPSHOT_DATE = /^\d{4}-\d{2}-\d{2}-\d{6}$/;

interface Disks {
  at: number;
  rows: DiskRow[];
  df: DfRow[];
}

let last: Disks | null = null;

async function runDf(): Promise<DfRow[]> {
  try {
    return parseDf((await wings.exec("df", ["-k", "-P"])).stdout);
  } catch {
    // Without df the disks still show, with no purgeable space and no other volumes.
    return [];
  }
}

/** Reads the disks now. */
export async function loadDisks(): Promise<DiskRow[]> {
  const [disks, df] = await Promise.all([wings.disks(), runDf()]);
  const rows = diskRows(disks, df);
  last = { at: Date.now(), rows, df };
  return rows;
}

/** The disks as read in the last `maxAgeMs`, reading them if that is older. */
export async function disksNow(maxAgeMs = 30_000): Promise<Disks> {
  if (!last || Date.now() - last.at > maxAgeMs) await loadDisks();
  return last!;
}

/** The dates of the local snapshots on the disk mounted at `mount`: none when `tmutil` can't list them. */
async function snapshotsOf(mount: string): Promise<string[]> {
  try {
    const { code, stdout } = await wings.exec("tmutil", ["listlocalsnapshotdates", mount]);
    return code === 0 ? stdout.split("\n").map((line) => line.trim()).filter((line) => SNAPSHOT_DATE.test(line)) : [];
  } catch {
    return [];
  }
}

/** The hidden space of the disk at `mount` for a scan that found `found` bytes, from numbers read now. Null when it isn't a disk. */
export async function hiddenOf(mount: string, found: number): Promise<Hidden | null> {
  await loadDisks();
  const { rows, df } = last!;
  const disk = rows.find((d) => d.mount === mount);
  return disk ? hiddenSpace(disk, availableOf(df, mount), found, await snapshotsOf(mount)) : null;
}
