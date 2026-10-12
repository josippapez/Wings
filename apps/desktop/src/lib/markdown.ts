export type Inline = { kind: "text" | "code" | "strong" | "em"; text: string } | { kind: "link"; text: string; href: string };

export type MdBlock =
  | { kind: "heading"; level: 1 | 2 | 3; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; items: string[] }
  | { kind: "code"; text: string }
  | { kind: "quote"; text: string }
  | { kind: "rule" };

/** Only https links are ever opened. Anything else renders as plain text. */
export const safeHref = (href: string) => (/^https:\/\/[^\s]+$/i.test(href) ? href : null);

const heading = /^(#{1,6})\s+(.*)$/;
const bullet = /^\s*[-*+]\s+(.*)$/;
const numbered = /^\s*\d+[.)]\s+(.*)$/;

/** A small subset of Markdown, as data: headings, paragraphs, flat lists, fenced code, quotes and rules. Never HTML. */
export function parseBlocks(src: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length) blocks.push({ kind: "paragraph", text: paragraph.join(" ") });
    paragraph = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*(```|~~~)/.test(line)) {
      flush();
      const code: string[] = [];
      while (++i < lines.length && !/^\s*(```|~~~)/.test(lines[i])) code.push(lines[i]);
      blocks.push({ kind: "code", text: code.join("\n") });
      continue;
    }
    const h = heading.exec(line);
    const li = bullet.exec(line) ?? numbered.exec(line);
    if (!line.trim()) flush();
    else if (h) {
      flush();
      blocks.push({ kind: "heading", level: Math.min(h[1].length, 3) as 1 | 2 | 3, text: h[2].replace(/\s+#+\s*$/, "") });
    } else if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flush();
      blocks.push({ kind: "rule" });
    } else if (li) {
      flush();
      const ordered = !bullet.test(line);
      const last = blocks.at(-1);
      if (last?.kind === "list" && last.ordered === ordered) last.items.push(li[1]);
      else blocks.push({ kind: "list", ordered, items: [li[1]] });
    } else if (line.startsWith(">")) {
      flush();
      const text = line.replace(/^>\s?/, "");
      const last = blocks.at(-1);
      if (last?.kind === "quote") last.text += ` ${text}`;
      else blocks.push({ kind: "quote", text });
    } else paragraph.push(line.trim());
  }
  flush();
  return blocks;
}

const inlinePattern = /!\[([^\]]*)\]\([^)\s]*\)|\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s][^*]*)\*/g;

/** Inline code, bold, italics and links. An image shows its alt text; a link that isn't https shows just its text. */
export function parseInline(src: string): Inline[] {
  const out: Inline[] = [];
  let at = 0;
  for (const m of src.matchAll(inlinePattern)) {
    if (m.index > at) out.push({ kind: "text", text: src.slice(at, m.index) });
    at = m.index + m[0].length;
    if (m[0].startsWith("!")) {
      if (m[1]) out.push({ kind: "text", text: m[1] });
    } else if (m[2] !== undefined) {
      const href = safeHref(m[3]);
      out.push(href ? { kind: "link", text: m[2], href } : { kind: "text", text: m[2] });
    } else if (m[4] !== undefined) out.push({ kind: "code", text: m[4] });
    else if (m[5] !== undefined) out.push({ kind: "strong", text: m[5] });
    else out.push({ kind: "em", text: m[6] });
  }
  if (at < src.length) out.push({ kind: "text", text: src.slice(at) });
  return out;
}
