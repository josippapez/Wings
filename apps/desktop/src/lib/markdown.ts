// A small, safe Markdown parser, as data, ported from packages/plugin-ui/markdown.js (same blocks, same bounded scans,
// same perf test). It never produces HTML: text stays text, so React escapes it, and the only URLs it lets through are
// https links and https, small data:image or this project's own stored images (`wings-image:<name>`).
//
// Blocks: headings, paragraphs, lists (flat, with an indent depth, and task items), fenced code, quotes, rules,
// GFM tables, and `<details>` with its `<summary>` as a fold. HTML comments are hidden. Inline: code, bold, italics, strikethrough, links, images and bare https URLs.


export type Align = "left" | "right" | "center" | null;

export type Inline =
  | { kind: "text" | "code" | "strong" | "em" | "del"; text: string }
  | { kind: "link"; text: string; href: string }
  | { kind: "image"; alt: string; src: string }
  | { kind: "local-image"; alt: string; name: string };

export type ListItem = { text: string; depth: number; checked: boolean | null };

export type MdBlock =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; items: ListItem[] }
  | { kind: "code"; lang: string; text: string }
  | { kind: "quote"; text: string }
  | { kind: "rule" }
  | { kind: "table"; align: Align[]; head: string[]; rows: string[][] }
  | { kind: "details"; summary: string; blocks: MdBlock[] };

/** A data: image longer than this, in characters, is shown as its alt text. */
export const MAX_DATA_IMAGE = 8000;

/** Only https links are ever opened. Anything else (javascript:, http:, data:, relative) is not a link. */
const httpsUrl = (url: string) => /^https:\/\/[^\s<>"'`]+$/i.test(url) && URL.canParse(url) && new URL(url).host !== "";

export const safeHref = (href: string): string | null => (httpsUrl(href) ? href : null);

/** An image kept in this project's storage: `wings-image:<file name>`. Only read from there, never fetched. */
export const LOCAL_IMAGE = /^wings-image:[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

/** Only https images, and small raster data: images. SVG is left out, since it can carry script. */
export const safeSrc = (src: string): string | null => {
  if (httpsUrl(src)) return src;
  if (LOCAL_IMAGE.test(src)) return src;
  if (src.length <= MAX_DATA_IMAGE && /^data:image\/(?:png|jpe?g|gif|webp);base64,[a-z0-9+/]+=*$/i.test(src)) return src;
  return null;
};

const heading = /^\s{0,3}(#{1,6})\s+(.*)$/;
const bullet = /^(\s*)[-*+]\s+(.*)$/;
const numbered = /^(\s*)\d+[.)]\s+(.*)$/;
const fence = /^\s*(```|~~~)\s*([\w+-]*)/;
const rule = /^\s*([-*_])(\s*\1){2,}\s*$/;
const delimiterCell = /^:?-+:?$/;
const detailsOpen = /^\s*<details(?:\s[^>]*)?>/i;
/** Folds nest at most this deep; a deeper `<details>` is text. */
const MAX_DETAILS_DEPTH = 3;

/** The cells of a table row: split on `|` that isn't escaped as `\|`, outer pipes dropped. */
export function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  const cells: string[] = [];
  let cell = "";
  for (let i = 0; i < s.length; i++) {
    if (s[i] === "\\" && s[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (s[i] === "|") {
      cells.push(cell.trim());
      cell = "";
    } else cell += s[i];
  }
  cells.push(cell.trim());
  return cells;
}

/** A table's delimiter row (`| --- | :-: |`) as column alignments, or null when the line isn't one. */
function delimiterRow(line: string): Align[] | null {
  if (!line.includes("-") || !line.includes("|") && !/^\s*:?-+:?\s*$/.test(line)) return null;
  const cells = splitRow(line);
  if (!cells.every((c) => delimiterCell.test(c))) return null;
  return cells.map((c) => (c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : c.startsWith(":") ? "left" : null));
}

/** A heading's text without its closing hashes (`## Title ##`), found by walking back from the end. */
function headingText(text: string): string {
  let end = text.length;
  while (end > 0 && /\s/.test(text[end - 1])) end--;
  let hashes = end;
  while (hashes > 0 && text[hashes - 1] === "#") hashes--;
  if (hashes < end && hashes > 0 && /\s/.test(text[hashes - 1])) {
    end = hashes;
    while (end > 0 && /\s/.test(text[end - 1])) end--;
  }
  return text.slice(0, end);
}

/**
 * Blocks of Markdown, as data:
 * `{ kind: "heading", level 1-6, text }`, `{ kind: "paragraph", text }`,
 * `{ kind: "list", ordered, items: { text, depth 0-3, checked: boolean | null }[] }`, `{ kind: "code", lang, text }`,
 * `{ kind: "quote", text }`, `{ kind: "rule" }` and `{ kind: "table", align, head, rows }`.
 */
export function parseBlocks(src: string, depth = 0): MdBlock[] {
  const blocks: MdBlock[] = [];
  const lines = String(src).replace(/\r\n?/g, "\n").split("\n");
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length) blocks.push({ kind: "paragraph", text: paragraph.join(" ") });
    paragraph = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const open = fence.exec(line);
    if (open) {
      flush();
      const code: string[] = [];
      while (++i < lines.length && !new RegExp(`^\\s*${open[1]}\\s*$`).test(lines[i])) code.push(lines[i]);
      blocks.push({ kind: "code", lang: open[2], text: code.join("\n") });
      continue;
    }
    // An HTML comment (`<!-- … -->`, often a bot's hidden state) shows nothing, as on the forge. Not inside inline
    // code. One left open hides the rest, as in HTML.
    const commentAt = line.indexOf("<!--");
    if (commentAt >= 0 && (line.slice(0, commentAt).match(/`/g)?.length ?? 0) % 2 === 0) {
      let rest = line.slice(commentAt + 4);
      let j = i;
      while (!rest.includes("-->") && j + 1 < lines.length) rest += `\n${lines[++j]}`;
      const close = rest.indexOf("-->");
      lines[j] = line.slice(0, commentAt) + (close >= 0 ? rest.slice(close + 3) : "");
      i = j - 1;
      continue;
    }
    if (!line.trim()) {
      flush();
      continue;
    }
    // A `<details>` after text on the same line ("…requirement. <details>"): the text stays in the paragraph and the
    // fold starts at the tag. Not inside inline code (an odd number of backticks before it).
    const tagAt = depth < MAX_DETAILS_DEPTH ? line.search(/<details(?:\s[^>]*)?>/i) : -1;
    if (tagAt > 0 && line.slice(0, tagAt).trim() && (line.slice(0, tagAt).match(/`/g)?.length ?? 0) % 2 === 0) {
      paragraph.push(line.slice(0, tagAt).trim());
      lines[i] = line.slice(tagAt);
      i--;
      continue;
    }
    // `<details>` up to its matching `</details>` (or the end), with the first `<summary>` as the fold's title.
    if (depth < MAX_DETAILS_DEPTH && detailsOpen.test(line)) {
      flush();
      const body = [line.replace(detailsOpen, "")];
      let open = 1 + (body[0].match(/<details(?:\s[^>]*)?>/gi)?.length ?? 0) - (body[0].match(/<\/details>/gi)?.length ?? 0);
      while (open > 0 && ++i < lines.length) {
        body.push(lines[i]);
        open += (lines[i].match(/<details(?:\s[^>]*)?>/gi)?.length ?? 0) - (lines[i].match(/<\/details>/gi)?.length ?? 0);
      }
      let inner = body.join("\n");
      const close = inner.toLowerCase().lastIndexOf("</details>");
      if (open <= 0 && close >= 0) inner = inner.slice(0, close);
      const summary = /<summary(?:\s[^>]*)?>([\s\S]{0,500}?)<\/summary>/i.exec(inner);
      if (summary) inner = inner.slice(0, summary.index) + inner.slice(summary.index + summary[0].length);
      blocks.push({ kind: "details", summary: summary ? summary[1].replace(/\s+/g, " ").trim() || "Details" : "Details", blocks: parseBlocks(inner, depth + 1) });
      continue;
    }
    // A table: a header row with a pipe, then a delimiter row with as many cells.
    if (line.includes("|") && i + 1 < lines.length) {
      const align = delimiterRow(lines[i + 1]);
      const head = align && splitRow(line);
      if (align && head && head.length === align.length) {
        flush();
        const rows: string[][] = [];
        let j = i + 2;
        for (; j < lines.length && lines[j].trim() && lines[j].includes("|"); j++) {
          const cells = splitRow(lines[j]);
          // Short rows are padded and long ones cut, as GFM does.
          rows.push(Array.from({ length: head.length }, (_, c) => cells[c] ?? ""));
        }
        blocks.push({ kind: "table", align, head, rows });
        i = j - 1;
        continue;
      }
    }
    const h = heading.exec(line);
    const li = bullet.exec(line) ?? numbered.exec(line);
    if (h) {
      flush();
      blocks.push({ kind: "heading", level: h[1].length, text: headingText(h[2]) });
    } else if (rule.test(line)) {
      flush();
      blocks.push({ kind: "rule" });
    } else if (li) {
      flush();
      const ordered = !bullet.test(line);
      const task = /^\[([ xX])\]\s+(.*)$/.exec(li[2]);
      const item = { text: task ? task[2] : li[2], depth: Math.min(3, Math.floor(li[1].replace(/\t/g, "  ").length / 2)), checked: task ? task[1] !== " " : null };
      const last = blocks.at(-1);
      if (last?.kind === "list" && last.ordered === ordered) last.items.push(item);
      else blocks.push({ kind: "list", ordered, items: [item] });
    } else if (line.trimStart().startsWith(">")) {
      flush();
      const text = line.replace(/^\s*>\s?/, "");
      const last = blocks.at(-1);
      if (last?.kind === "quote") last.text += ` ${text}`;
      else blocks.push({ kind: "quote", text });
    } else paragraph.push(line.trim());
  }
  flush();
  return blocks;
}

// Every scan is bounded and stops at a newline, so a long run of brackets or markers costs a bounded step per start
// rather than a pass over the rest of the text.
const inlinePattern =
  /\[!\[([^\[\]\n]{0,200})\]\(([^)\s]{0,2000})\)\]\(([^)\s]{1,2000})\)|!\[([^\[\]\n]{0,200})\]\(([^)\s]{0,2000})\)|\[([^\[\]\n]{1,200})\]\(([^)\s]{1,2000})\)|`([^`\n]{1,2000})`|\*\*([^*\n]{1,2000})\*\*|~~([^~\n]{1,2000})~~|\*([^*\s][^*\n]{0,2000})\*|(https:\/\/[^\s<>"'`)\]]{1,2000})/gi;

const sentenceEnd = ".,;:!?";

/**
 * Inline spans: `{ kind: "text" | "code" | "strong" | "em" | "del", text }`, `{ kind: "link", text, href }` and
 * `{ kind: "image", alt, src }`. A link that isn't https is its text; an image that isn't allowed is its alt text. An
 * image inside a link (`[![alt](img)](url)`) is a link whose text is the alt.
 */
export function parseInline(src: string): Inline[] {
  const out: Inline[] = [];
  let at = 0;
  for (const m of src.matchAll(inlinePattern)) {
    if (m.index > at) out.push({ kind: "text", text: src.slice(at, m.index) });
    at = m.index + m[0].length;
    if (m[3] !== undefined) {
      const href = safeHref(m[3]);
      const text = m[1] || m[3];
      out.push(href ? { kind: "link", text, href } : { kind: "text", text: m[1] });
    } else if (m[5] !== undefined) {
      const safe = safeSrc(m[5]);
      if (safe && LOCAL_IMAGE.test(safe)) out.push({ kind: "local-image", alt: m[4], name: safe.slice("wings-image:".length) });
      else if (safe) out.push({ kind: "image", alt: m[4], src: safe });
      else if (m[4]) out.push({ kind: "text", text: m[4] });
    } else if (m[7] !== undefined) {
      const href = safeHref(m[7]);
      out.push(href ? { kind: "link", text: m[6], href } : { kind: "text", text: m[6] });
    } else if (m[8] !== undefined) out.push({ kind: "code", text: m[8] });
    else if (m[9] !== undefined) out.push({ kind: "strong", text: m[9] });
    else if (m[10] !== undefined) out.push({ kind: "del", text: m[10] });
    else if (m[11] !== undefined) out.push({ kind: "em", text: m[11] });
    else {
      // A bare URL, without the punctuation that ends a sentence.
      let end = m[12].length;
      while (end > 0 && sentenceEnd.includes(m[12][end - 1])) end--;
      const url = m[12].slice(0, end);
      const href = safeHref(url);
      out.push(href ? { kind: "link", text: url, href } : { kind: "text", text: url });
      if (end < m[12].length) out.push({ kind: "text", text: m[12].slice(end) });
    }
  }
  if (at < src.length) out.push({ kind: "text", text: src.slice(at) });
  return out;
}
