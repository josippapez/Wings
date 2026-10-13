// Notes live in wings.storage, one key per project id. Only the main script writes them: pages send it saves, so an
// agent's append and a page's save never overwrite each other.

/**
 * Per project. A page's save travels as a broadcast, which Wings limits to 64,000 characters of JSON, and JSON can
 * double a text of quotes and newlines. All of the plugin's storage is limited to 1 MB.
 */
export const MAX_CHARS = 30_000;

export interface Notes {
  text: string;
  updatedAt: number;
}

/** A page asks main to save. `base` is the text the page last got from storage, `rid` names the request. */
export interface SaveMessage {
  type: "save";
  projectId: string;
  base: string;
  text: string;
  rid: string;
}

/** Main tells every page that a project's notes changed, so they read them again. `error` answers a failed save. */
export interface NotesMessage {
  type: "notes";
  projectId: string;
  rid?: string;
  error?: string;
}

const key = (id: string) => `notes:${id}`;

export async function readNotes(projectId: string): Promise<Notes> {
  const value = (await wings.storage.get(key(projectId))) as Partial<Notes> | null;
  return { text: typeof value?.text === "string" ? value.text : "", updatedAt: typeof value?.updatedAt === "number" ? value.updatedAt : 0 };
}

export async function writeNotes(projectId: string, text: string, name: string): Promise<void> {
  if (text.length > MAX_CHARS) throw new Error(`The notes for ${name} are limited to ${MAX_CHARS.toLocaleString("en")} characters. Trim them first.`);
  await wings.storage.set(key(projectId), { text, updatedAt: Date.now() } satisfies Notes);
}

/** `current` with a timestamped section added after it. `current` stays a prefix of the result. */
export function appendSection(current: string, text: string, now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
  const section = `## ${stamp} (Claude)\n\n${text.trim()}\n`;
  if (!current.trim()) return section;
  return current + (current.endsWith("\n\n") ? "" : current.endsWith("\n") ? "\n" : "\n\n") + section;
}

/** What a page's save becomes when storage moved since the page read it: sections added meanwhile stay. */
export function mergeSave(current: string, base: string, text: string): string {
  return current !== base && current.startsWith(base) ? text + current.slice(base.length) : text;
}
