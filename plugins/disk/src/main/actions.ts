// What the tab asks main to do on the disk: move the Collector to the Trash, show a file, and manage the local snapshots.

import { mapLimit } from "wings-plugin-sdk/async";

import { ch, type Collected, type TrashResult } from "../lib/messages";
import { protectedReason } from "../lib/protect";
import { collectorNow, uncollect } from "./collector";
import { SNAPSHOT_DATE, disksNow } from "./disks";
import { recomputeHidden, removeFromScans } from "./scans";

type Exec = Awaited<ReturnType<Wings["exec"]>>;

/** What a command printed, trimmed: its errors, else its output. */
const said = ({ stdout, stderr }: Exec) => stderr.trim() || stdout.trim();

/** An Error for a command that didn't succeed. */
const failure = (program: string, out: Exec) => new Error(said(out) || `${program} failed${out.code === null ? "" : ` with code ${out.code}`}.`);

/** Anything that is handed to a program as a path must be one, and so can't be taken for a flag. */
function assertFullPath(path: string) {
  if (!path.startsWith("/")) throw new Error(`${path} isn't a full path.`);
}

/**
 * Why `trash` failed, as it says it: it wraps the reason in a time stamp, its process id and a dump of the error
 * (`... trash[123:456] # Error attempting to move /x to the trash folder, Error Domain=NSCocoaErrorDomain Code=4 "The file
 * “x” doesn’t exist." UserInfo={...}`).
 */
function trashReason(out: Exec): string {
  const text = said(out);
  const reason = /Error Domain=\S+ Code=-?\d+ "([^"]+)"/.exec(text)?.[1];
  return reason ?? (text.replace(/^\d{4}-\d\d-\d\d [\d:.]+ trash\[[\d:]+\] /, "") || failure("trash", out).message);
}

let trashing: Promise<TrashResult> | null = null;

/** Moves everything in the Collector to the Trash, and gives what moved and what didn't. A second call while one runs gets that one's result. */
export function trashCollected(): Promise<TrashResult> {
  trashing ??= moveToTrash().finally(() => (trashing = null));
  return trashing;
}

async function moveToTrash(): Promise<TrashResult> {
  const { items } = await collectorNow();
  const mounts = (await disksNow()).rows.map((d) => d.mount);
  // Each item is checked again where it matters most, so one stored by an older version can't get past what is refused now.
  const errors = await mapLimit(items, 4, async (item): Promise<string | null> => {
    const reason = protectedReason(item.path, mounts);
    if (reason) return reason;
    try {
      const out = await wings.exec("trash", [item.path]);
      return out.code === 0 ? null : trashReason(out);
    } catch (error) {
      return error instanceof Error ? error.message : String(error);
    }
  });
  const trashed: Collected[] = [];
  const failed: TrashResult["failed"] = [];
  items.forEach((item, i) => {
    const message = errors[i];
    if (message === null) trashed.push(item);
    else failed.push({ path: item.path, message });
  });
  removeFromScans(trashed);
  await uncollect(trashed.map((item) => item.path));
  ch.refresh("disks");
  return { trashed: trashed.map((item) => item.path), failed, freed: trashed.reduce((sum, item) => sum + item.size, 0) };
}

/** Shows `path` in Finder. */
export async function reveal(path: string): Promise<void> {
  assertFullPath(path);
  const out = await wings.exec("open", ["-R", path]);
  if (out.code !== 0) throw failure("open", out);
}

/** Opens Quick Look on `path` and returns: it stays open until the user closes it. */
export function preview(path: string): void {
  assertFullPath(path);
  // exec ends Quick Look after 5 minutes at most, which rejects it: that is expected, and nothing to report.
  wings.exec("qlmanage", ["-p", path], { timeoutMs: 300_000 }).catch(() => {});
}

/** The disks, and the hidden space of the scanned disks, once space was freed. */
async function spaceFreed() {
  ch.refresh("disks");
  await recomputeHidden();
}

export async function deleteSnapshot(date: string): Promise<void> {
  if (!SNAPSHOT_DATE.test(date)) throw new Error(`${date} isn't the date of a local snapshot.`);
  const out = await wings.exec("tmutil", ["deletelocalsnapshots", date]);
  if (out.code !== 0) throw failure("tmutil", out);
  await spaceFreed();
}

/** Asks macOS to free the purgeable space of the disk at `mount` by thinning its local snapshots. It can take minutes. */
export async function purge(mount: string): Promise<void> {
  if (!(await disksNow()).rows.some((d) => d.mount === mount)) throw new Error(`${mount} isn't a disk.`);
  const out = await wings.exec("tmutil", ["thinlocalsnapshots", mount, "999999999999", "4"], { timeoutMs: 300_000 });
  if (out.code !== 0) throw failure("tmutil", out);
  await spaceFreed();
}
