// The files an agent's change journal reports for an issue: one row per path, newest first, bounded.

import { isAgent } from "./actor";
import { LIMITS, type Actor, type FileChange } from "./domain";

const cut = (text: string, max: number) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

/** `old` with `incoming` upserted by path as `by` at `now`: the newest description, time and session win, and past `LIMITS.files` the oldest rows go. */
export function upsertFiles(old: FileChange[] | undefined, incoming: { path: string; what: string }[], by: Actor, now: number): FileChange[] {
  const session = isAgent(by) && by.id ? { id: by.id, ...(by.name ? { name: by.name } : {}), ...(by.kind ? { kind: by.kind } : {}) } : undefined;
  const fresh = new Map<string, FileChange>();
  for (const f of incoming) {
    const path = cut(f.path.trim(), LIMITS.filePath);
    if (path) fresh.set(path, { path, what: cut(f.what.trim(), LIMITS.fileWhat), at: now, ...(session ? { session } : {}) });
  }
  const kept = (old ?? []).filter((f) => !fresh.has(f.path));
  return [...fresh.values(), ...kept].sort((a, b) => b.at - a.at).slice(0, LIMITS.files);
}
