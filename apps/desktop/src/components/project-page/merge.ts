/** What a section editor holds, apart from the textarea itself. */
export type EditorState = {
  /** The section's text as storage last confirmed it, to tell an agent's append from your own typing. */
  base: string;
  /** What is in the textarea. */
  text: string;
  /** Typed since the last save went out. */
  dirty: boolean;
  /** A save is out and not answered yet. */
  inflight: boolean;
  /** Whether this editor ever sent a save. After that the stored text can lag behind what it saved, so only an extension of `base` is taken. */
  sent: boolean;
  /** The stored text when the last save went out: what it shows until it catches up with that save. */
  stale: string;
};

/**
 * What to do when storage reports `stored` for the section being edited, or null to leave the editor alone.
 * An agent's append goes after what you typed and never replaces it; an older text (the stored value catching up with a
 * save this editor made) is ignored; and nothing is taken while a save is out, since its answer brings the rest.
 */
export function takeStored(s: EditorState, stored: string): { text: string; base: string } | null {
  if (s.inflight || stored === s.base || (s.sent && stored === s.stale)) return null;
  const extension = stored.startsWith(s.base);
  if (s.sent && !extension) return null;
  if (!s.dirty) return { text: stored, base: stored };
  if (extension) return { text: s.text + stored.slice(s.base.length), base: stored };
  return null;
}

/** A title for a new section that no other section has: "Untitled", "Untitled 2"... Titles compare ignoring case. */
export function freshTitle(taken: string[], base = "Untitled") {
  const used = new Set(taken.map((t) => t.trim().toLowerCase()));
  for (let n = 1; ; n++) {
    const title = n === 1 ? base : `${base} ${n}`;
    if (!used.has(title.toLowerCase())) return title;
  }
}

/** The fact with this key, ignoring case and surrounding spaces, as storage matches keys. */
export const findFact = <F extends { key: string }>(facts: F[], key: string) => facts.find((f) => f.key.trim().toLowerCase() === key.trim().toLowerCase());

const IMAGE_EXT: Record<string, string> = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" };

/** The file extension `addImage` takes for an image's MIME type, or null for a type that isn't stored (SVG can carry script). */
export const imageExt = (mime: string) => IMAGE_EXT[mime.toLowerCase()] ?? null;

/** The Markdown for a stored image, from what `addImage` returned: its name, or already the `wings-image:` reference. */
export const imageMarkdown = (stored: string) => `![](${stored.startsWith("wings-image:") ? stored : `wings-image:${stored}`})`;
