export type NoteBlock = { list: true; items: string[] } | { list: false; text: string };

/** Release notes are plain text: lines starting with "- " become list items (consecutive ones share a list), anything else a paragraph. Never HTML. */
export function parseNotes(body: string | undefined): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  for (const line of (body ?? "").split("\n").map((l) => l.trim())) {
    if (!line) continue;
    const last = blocks.at(-1);
    if (line.startsWith("- ")) {
      const item = line.slice(2).trim();
      if (last?.list) last.items.push(item);
      else blocks.push({ list: true, items: [item] });
    } else {
      blocks.push({ list: false, text: line });
    }
  }
  return blocks;
}
