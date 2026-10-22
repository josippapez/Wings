// The scans. Main runs every one and keeps the results, so each Disk usage tab and Claude's tools see the same ones, and puts
// each change on the channel. Nothing here touches `wings` before it is called: Claude's tools import this file in Node.

import { errorMessage } from "wings-plugin-sdk/async";

import { graft, removePath } from "../lib/edit";
import { ch, type Collected, type Entry, type Scan, type ScanSummary } from "../lib/messages";
import { find } from "../lib/tree";
import { disksNow, hiddenOf } from "./disks";
import { expandHome, homeFolder, tidy } from "./paths";

export type DoneScan = Extract<Scan, { state: "done" }>;
type Scanning = Extract<Scan, { state: "scanning" }>;

/** The scans by the root a tab or a tool asked for, like `~` or `/`: `scan.root` is that string, and the tree's name the folder's full path. */
export const scans = new Map<string, Scan>();

/** What Wings keeps of a scan: entries of at least a floor, at most this many. */
const MAX_ENTRIES = 30_000;
/** The scan's own message when it is stopped. */
const STOPPED = "The scan was stopped";
/** When a root last changed state, for the summaries of a scan that has no time of its own. */
const stamps = new Map<string, number>();
/** The bytes moved to the Trash out of a done scan since it ran: still used on the disk, so not hidden. */
const moved = new WeakMap<DoneScan, number>();

interface Job {
  controller: AbortController;
  /** Settles once the scan shows as scanning, or has ended without. */
  started: Promise<void>;
  /** Settles when the scan has ended, in any state. Never rejects. */
  done: Promise<void>;
}
const jobs = new Map<string, Job>();
/** Folders being scanned in more detail, by root and path. */
const refinements = new Map<string, Promise<void>>();

export const summaries = (): ScanSummary[] =>
  [...scans.values()]
    .map((scan) => ({
      root: scan.root,
      state: scan.state,
      size: scan.state === "done" ? scan.tree.size : null,
      at: scan.state === "done" ? scan.at : scan.state === "scanning" ? scan.startedAt : (stamps.get(scan.root) ?? 0),
    }))
    .sort((a, b) => b.at - a.at);

/** Keeps `scan` for `root`, or drops it with null, and tells every frame. `quiet` leaves the summaries alone: progress changes none. */
function keep(root: string, scan: Scan | null, quiet = false) {
  if (scan) scans.set(root, scan);
  else scans.delete(root);
  ch.put("scan", { root }, scan);
  if (quiet) return;
  if (scan) stamps.set(root, Date.now());
  else stamps.delete(root);
  ch.put("scans", undefined, summaries());
}

/** The finished scans that hold `path` in their trees, newest first, each with the entries from its root down to `path`. */
export function holders(path: string, maxAgeMs = Infinity) {
  const found: { scan: DoneScan; trail: Entry[] }[] = [];
  for (const scan of scans.values()) {
    if (scan.state !== "done" || Date.now() - scan.at > maxAgeMs) continue;
    const trail = find(scan.tree, path);
    if (trail) found.push({ scan, trail });
  }
  return found.sort((a, b) => b.scan.at - a.scan.at);
}

/** `path` made absolute with the home folder the scans know, tidied; null for a `~` path before any scan of one is done. */
export const resolve = (path: string) => expandHome(path, homeFolder(scans.values()));

/** The least size to keep when scanning a folder of `size` bytes in detail: about 40,000 entries' worth, and nothing under 4 KB. */
const detailFor = (size: number) => Math.max(4096, Math.round(size / 40_000));

/** What a scan of `root` is told: the bytes it should find, the least size to keep, and the disk when `root` is one's mount. */
async function planOf(root: string) {
  const { rows } = await disksNow();
  const mount = tidy(root);
  const disk = rows.find((d) => d.mount === mount);
  if (disk) {
    const expected = disk.used + disk.purgeable;
    return { disk, expected, minBytes: Math.max(65_536, Math.round(expected / 40_000)) };
  }
  const full = resolve(root);
  const known = full === null ? undefined : holders(full)[0]?.trail.at(-1)?.size;
  if (known === undefined) return { disk: undefined, expected: null, minBytes: 1_048_576 };
  return { disk: undefined, expected: known, minBytes: detailFor(known) };
}

async function run(root: string, job: Job, begin: () => void): Promise<void> {
  const { signal } = job.controller;
  // A job that is stopped can be replaced by a new scan of the root before it ends: it then leaves the root alone.
  const current = () => jobs.get(root) === job;
  try {
    const plan = await planOf(root);
    if (signal.aborted) throw new Error(STOPPED);
    let scanning: Scanning = { state: "scanning", root, progress: { entries: 0, bytes: 0, path: "" }, expected: plan.expected, startedAt: Date.now() };
    keep(root, scanning);
    begin();
    const result = await wings.diskUsage(root, {
      minBytes: plan.minBytes,
      maxEntries: MAX_ENTRIES,
      signal,
      // A progress that arrives after the scan ended, or was stopped, must not bring its state back.
      onProgress: (progress) => {
        if (scans.get(root) === scanning) keep(root, (scanning = { ...scanning, progress }), true);
      },
    });
    const hidden = plan.disk
      ? await hiddenOf(plan.disk.mount, result.root.size).catch((error) => {
          // The tree is the result: a disk whose snapshots or numbers can't be read shows without its hidden space.
          console.error(error);
          return null;
        })
      : null;
    if (signal.aborted) throw new Error(STOPPED);
    const { floor, entries, restricted, ms } = result;
    keep(root, { state: "done", root, tree: result.root, floor, entries, restricted, ms, at: Date.now(), disk: plan.disk?.mount ?? null, hidden, refining: [] });
  } catch (error) {
    if (!current()) return;
    if (signal.aborted || errorMessage(error) === STOPPED) keep(root, null);
    else keep(root, { state: "error", root, message: errorMessage(error) });
  } finally {
    begin();
    if (current()) jobs.delete(root);
  }
}

/** Scans `root`, or gives the scan of it that is running. */
export function startScan(root: string): Job {
  const running = jobs.get(root);
  if (running && !running.controller.signal.aborted) return running;
  const controller = new AbortController();
  let begin!: () => void;
  const started = new Promise<void>((resolve) => (begin = resolve));
  const job: Job = { controller, started, done: Promise.resolve() };
  jobs.set(root, job);
  job.done = run(root, job, begin);
  return job;
}

export function stopScan(root: string): void {
  jobs.get(root)?.controller.abort();
}

export function forgetScan(root: string): void {
  jobs.get(root)?.controller.abort();
  keep(root, null);
}

/**
 * Scans `root`, waits for the scan, and gives it done, or throws why it isn't. The tool that asked hears why, so a scan that
 * failed and was begun here, like one of a path Claude mistyped, isn't left in the tab's list.
 */
export async function scanFolder(root: string): Promise<DoneScan> {
  const running = jobs.get(root);
  const job = startScan(root);
  const begun = job !== running;
  await job.done;
  const scan = scans.get(root);
  if (scan?.state === "done") return scan;
  if (scan?.state === "error" && begun) forgetScan(root);
  throw new Error(scan?.state === "error" ? scan.message : `The scan of ${root} was stopped.`);
}

/** Scans the folder at `path` of the done scan `root` again in more detail, and puts the result in its tree. */
export function refine(root: string, path: string): Promise<void> {
  const key = `${root}\0${path}`;
  const running = refinements.get(key);
  if (running) return running;
  const scan = scans.get(root);
  if (scan?.state !== "done") return Promise.resolve();
  const entry = find(scan.tree, path)?.at(-1);
  if (!entry || entry.kind !== "dir" || entry.restricted) return Promise.resolve();
  const job = deepen(root, scan, path, entry).finally(() => refinements.delete(key));
  refinements.set(key, job);
  return job;
}

async function deepen(root: string, scan: DoneScan, path: string, entry: Entry): Promise<void> {
  scan.refining = [...scan.refining, path];
  keep(root, scan, true);
  try {
    const result = await wings.diskUsage(path, { minBytes: detailFor(entry.size), maxEntries: MAX_ENTRIES });
    // A scan that was forgotten or run again while this one ran isn't the tree this result is for.
    if (scans.get(root) === scan) graft(scan.tree, path, result.root, result.floor);
  } finally {
    if (scans.get(root) === scan) {
      scan.refining = scan.refining.filter((p) => p !== path);
      keep(root, scan);
    }
  }
}

/** Every entry directly in the folder at `path`, small ones included. */
export async function listAll(path: string): Promise<Entry> {
  return (await wings.diskUsage(path, { minBytes: 0, depth: 1, maxEntries: 20_000 })).root;
}

/** How small the entries under `trail`'s last one are kept: by its scan's floor, or by the nearest folder above it, itself included, that was scanned again in more detail. */
const detailOf = (scan: DoneScan, trail: Entry[]) => trail.findLast((e) => e.detail !== undefined)?.detail ?? scan.floor;

/** Whether the folder at the end of `trail` was kept down to a floor that hides most of what is in it, and a scan of it alone would do better. It is scanned again once at most. */
function coarse(scan: DoneScan, trail: Entry[]): boolean {
  const entry = trail[trail.length - 1];
  const floor = detailOf(scan, trail);
  return entry.kind === "dir" && !entry.restricted && entry.detail === undefined && floor * 2000 > entry.size && detailFor(entry.size) < floor;
}

/** The entry at the end of `trail`, in the scan `scan` and at `at`, scanned in more detail first when it is coarse. */
async function sharpened(scan: DoneScan, trail: Entry[], at: string) {
  if (coarse(scan, trail)) await refine(scan.root, at);
  return { scan, entry: trail[trail.length - 1], path: at, detail: detailOf(scan, trail) };
}

/**
 * The entry at `path` for a tool, scanned in more detail first when it is coarse: from a finished scan under `maxAgeMs`
 * old that holds it, the finest first; or else from a scan of `path` itself, which then shows in the tab too. `fresh`
 * always scans. `detail` is the size under which what is in the entry isn't kept one by one.
 */
export async function entryAt(path: string, { fresh = false, maxAgeMs = 30 * 60_000 } = {}) {
  const full = resolve(path);
  if (full !== null && !fresh) {
    const hit = holders(full, maxAgeMs).sort((a, b) => detailOf(a.scan, a.trail) - detailOf(b.scan, b.trail))[0];
    if (hit) return sharpened(hit.scan, hit.trail, full);
  }
  const scan = await scanFolder(tidy(path));
  return sharpened(scan, [scan.tree], scan.tree.name);
}

/** Takes what moved to the Trash out of every done scan. */
export function removeFromScans(items: Collected[]): void {
  for (const [root, scan] of scans) {
    if (scan.state !== "done") continue;
    let touched = false;
    for (const { path, size } of items) {
      const before = scan.tree.size;
      const how = removePath(scan.tree, path, size);
      if (how === "root") {
        // The folder the scan is of is in the Trash now.
        keep(root, null);
        touched = false;
        break;
      }
      if (how) {
        moved.set(scan, (moved.get(scan) ?? 0) + before - scan.tree.size);
        touched = true;
      }
    }
    if (touched) keep(root, scan);
  }
}

/** Works the hidden space of every done disk scan out again, from the disks as they are now. */
export async function recomputeHidden(): Promise<void> {
  for (const [root, scan] of [...scans]) {
    if (scan.state !== "done" || !scan.disk) continue;
    const hidden = await hiddenOf(scan.disk, scan.tree.size + (moved.get(scan) ?? 0));
    if (scans.get(root) === scan) {
      scan.hidden = hidden;
      keep(root, scan, true);
    }
  }
}
