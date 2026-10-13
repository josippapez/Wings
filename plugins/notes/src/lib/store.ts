// Project info in wings.storage, one key per project id. Pages read it; only main writes it.

import { fromLegacy, parseInfo, type Info } from "./model";

const infoKey = (id: string) => `info:${id}`;
/** Where the free-form notes of 0.1.x lived: `{ text, updatedAt }`. Moved into a "Notes" section once, then deleted. */
const legacyKey = (id: string) => `notes:${id}`;

async function legacy(projectId: string): Promise<Info | null> {
  const old = (await wings.storage.get(legacyKey(projectId))) as { text?: unknown; updatedAt?: unknown } | null;
  return typeof old?.text === "string" && old.text.trim() ? fromLegacy(old.text, typeof old.updatedAt === "number" ? old.updatedAt : Date.now()) : null;
}

/** A value is there but isn't info this version reads (damaged, or written by a newer version). It is never overwritten. */
const unreadable = () =>
  new Error("The stored info for this project can't be read, so it is left as it is and not changed. It may have been written by a newer version of this plugin.");

/** Stored info; null when nothing is stored; throws when something is stored that isn't readable info. */
async function stored(projectId: string): Promise<Info | null> {
  const raw = await wings.storage.get(infoKey(projectId));
  if (raw == null) return null;
  const info = parseInfo(raw);
  if (!info) throw unreadable();
  return info;
}

/** What a page shows. Old notes not yet moved by main show as the section they'll become; nothing is written. */
export async function readInfo(projectId: string): Promise<Info | null> {
  return (await stored(projectId)) ?? (await legacy(projectId));
}

/** For main: like `readInfo`, but moves old notes into the new key first. */
export async function loadInfo(projectId: string): Promise<Info | null> {
  const found = await stored(projectId);
  if (found) return found;
  const migrated = await legacy(projectId);
  if (migrated) await saveInfo(projectId, migrated);
  await wings.storage.delete(legacyKey(projectId));
  return migrated;
}

export const saveInfo = (projectId: string, info: Info) => wings.storage.set(infoKey(projectId), info);

/** Project ids that still have old notes. */
export async function legacyProjects(): Promise<string[]> {
  return (await wings.storage.keys()).filter((k) => k.startsWith("notes:")).map((k) => k.slice("notes:".length));
}
