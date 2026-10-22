// The Collector: what the user, or Claude, has gathered to move to the Trash. It is kept in `wings.storage`, so it is still
// there after a restart, and put on the channel on every change.

import { serial } from "wings-plugin-sdk/async";

import { ch, type Collected, type Collector } from "../lib/messages";
import { protectedReason } from "../lib/protect";
import { disksNow } from "./disks";
import { inside, tidy } from "./paths";

const KINDS = new Set<string>(["dir", "file", "link", "other"]);

let items: Collected[] = [];
let loaded: Promise<void> | null = null;
/** Changes run one at a time, so two clicks never save over each other. */
const write = serial();

/** What storage holds, as far as it is a list of collected items. */
function clean(value: unknown): Collected[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((v): v is Collected => typeof v?.path === "string" && Number.isFinite(v.size) && KINDS.has(v.kind))
    .map(({ path, size, kind }) => ({ path, size, kind }));
}

/** Reads the Collector from storage, once. */
export function loadCollector(): Promise<void> {
  loaded ??= wings.storage.get("collector").then(
    (stored) => void (items = clean(stored)),
    (error) => {
      loaded = null;
      throw error;
    },
  );
  return loaded;
}

export async function collectorNow(): Promise<Collector> {
  await loadCollector();
  return { items, total: items.reduce((sum, i) => sum + i.size, 0) };
}

async function save() {
  await wings.storage.set("collector", items);
  ch.put("collector", undefined, await collectorNow());
}

/** Adds `item` unless it, or a folder that holds it, is collected. A folder takes out what it holds. */
function add(item: Collected): boolean {
  if (items.some((c) => c.path === item.path || (c.kind === "dir" && inside(item.path, c.path)))) return false;
  items = [...items.filter((c) => !inside(c.path, item.path)), item];
  return true;
}

/** Adds what may be collected, and says what it added and what it refused, with why. */
export async function collect(incoming: Collected[]): Promise<{ added: Collected[]; refused: { path: string; reason: string }[] }> {
  await loadCollector();
  const mounts = (await disksNow()).rows.map((d) => d.mount);
  return write(async () => {
    const made: Collected[] = [];
    const refused: { path: string; reason: string }[] = [];
    for (const { path, size, kind } of incoming) {
      const reason = protectedReason(path, mounts);
      const item = { path: tidy(path), size, kind };
      if (reason) refused.push({ path, reason });
      else if (add(item)) made.push(item);
    }
    // A folder added later in the same call takes out what an earlier item of it added.
    const added = made.filter((item) => items.includes(item));
    if (made.length) await save();
    return { added, refused };
  });
}

/** Takes the items at `paths` out of the Collector. */
export async function uncollect(paths: string[]): Promise<void> {
  await loadCollector();
  await write(async () => {
    const gone = new Set(paths.map(tidy));
    items = items.filter((c) => !gone.has(c.path));
    await save();
  });
}
