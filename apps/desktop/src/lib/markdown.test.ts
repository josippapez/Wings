/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_DATA_IMAGE, parseBlocks, parseInline, safeHref, safeSrc } from "./markdown.ts";

test("headings, paragraphs, lists and fenced code come out as blocks", () => {
  assert.deepEqual(parseBlocks("# Title\nsome\ntext\n\n- a\n- b\n\n1. one\n\n```sh\nls <b>\n```"), [
    { kind: "heading", level: 1, text: "Title" },
    { kind: "paragraph", text: "some text" },
    { kind: "list", ordered: false, items: [{ text: "a", depth: 0, checked: null }, { text: "b", depth: 0, checked: null }] },
    { kind: "list", ordered: true, items: [{ text: "one", depth: 0, checked: null }] },
    { kind: "code", lang: "sh", text: "ls <b>" },
  ]);
});

test("a GFM table has a head, alignment and padded rows", () => {
  const [t] = parseBlocks("| Name | Port | Note |\n| :--- | ---: | :-: |\n| dev | 5173 |\n| a \\| b | 1 | 2 | 3 |");
  assert.deepEqual(t, { kind: "table", align: ["left", "right", "center"], head: ["Name", "Port", "Note"], rows: [["dev", "5173", ""], ["a | b", "1", "2"]] });
});

test("a pipe line without a delimiter row is a paragraph", () => {
  assert.deepEqual(parseBlocks("a | b\nc | d").map((b) => b.kind), ["paragraph"]);
});

test("task items and depth", () => {
  const [l] = parseBlocks("- [x] done\n- [ ] todo\n  - nested\n- plain");
  assert.equal(l.kind, "list");
  if (l.kind === "list") assert.deepEqual(l.items.map((i) => [i.text, i.depth, i.checked]), [["done", 0, true], ["todo", 0, false], ["nested", 1, null], ["plain", 0, null]]);
});

test("only https links stay links", () => {
  for (const bad of ["javascript:alert(1)", "http://example.com", "data:text/html,x", "//example.com", "/relative", "JaVaScRiPt:alert(1)", 'https://x.com/a"onclick=1']) {
    assert.equal(safeHref(bad), null, bad);
    assert.ok(!parseInline(`[x](${bad})`).some((p) => p.kind === "link"), bad);
  }
  assert.equal(safeHref("https://example.com/a"), "https://example.com/a");
  assert.deepEqual(parseInline("see [docs](https://x.dev) and [bad](javascript:x) `ls`"), [
    { kind: "text", text: "see " },
    { kind: "link", text: "docs", href: "https://x.dev" },
    { kind: "text", text: " and " },
    { kind: "text", text: "bad" },
    { kind: "text", text: " " },
    { kind: "code", text: "ls" },
  ]);
});

test("bare https URLs become links without the trailing full stop", () => {
  assert.deepEqual(parseInline("see https://example.com/x."), [{ kind: "text", text: "see " }, { kind: "link", text: "https://example.com/x", href: "https://example.com/x" }, { kind: "text", text: "." }]);
});

test("images: https, small raster data URLs and this project's own files only", () => {
  const png = "data:image/png;base64,iVBORw0KGgo=";
  assert.equal(safeSrc(png), png);
  assert.deepEqual(parseInline("![logo](https://example.com/a.png)"), [{ kind: "image", alt: "logo", src: "https://example.com/a.png" }]);
  assert.deepEqual(parseInline("![shot](wings-image:a1b2.png)"), [{ kind: "local-image", alt: "shot", name: "a1b2.png" }]);
  for (const bad of ["http://example.com/a.png", "javascript:alert1", "data:image/svg+xml;base64,PHN2Zz4=", "data:text/html;base64,PGI+", "data:image/png;base64," + "A".repeat(MAX_DATA_IMAGE), "file:///etc/passwd", "/a.png", "wings-image:../x.png", "wings-image:a/b.png", "wings-image:"]) {
    assert.equal(safeSrc(bad), null, bad.slice(0, 30));
    const parts = parseInline(`![alt text](${bad})`);
    assert.ok(!parts.some((p) => p.kind === "image" || p.kind === "local-image"), bad.slice(0, 30));
    assert.ok(parts.some((p) => p.kind === "text" && p.text.includes("alt text")), bad.slice(0, 30));
  }
});

test("a linked image is a link whose text is the image's alt, never a link to the image", () => {
  assert.deepEqual(parseInline("[![logo](https://img.example/a.png)](https://example.com/x)"), [{ kind: "link", text: "logo", href: "https://example.com/x" }]);
  assert.deepEqual(parseInline("[![logo](https://img.example/a.png)](javascript:alert1)"), [{ kind: "text", text: "logo" }]);
});

test("pathological 40k inputs parse in under 100 ms", () => {
  const n = 40_000;
  const inputs: Record<string, string> = {
    "open brackets": "[".repeat(n),
    "open image brackets": "![".repeat(n / 2),
    "bracket then no paren": "[" + "a".repeat(n),
    "link text without close": "[a](" + "b".repeat(n),
    "dots after a url": "https://a.b/" + ".".repeat(n),
    "dots then x": "https://a.b/" + "!".repeat(n) + "x",
    "heading spaces": "# a" + " ".repeat(n) + "x",
    "heading hashes": "# a " + "#".repeat(n) + " x",
    backticks: "`a ".repeat(n / 3),
    stars: "*a ".repeat(n / 3),
    "double stars": "**a ".repeat(n / 4),
    tildes: "~~a ".repeat(n / 4),
    "rule-ish": "- ".repeat(n / 2) + "x",
    "table pipes": "|".repeat(n) + "\n|" + "-|".repeat(n / 2),
    "many lines of brackets": "[a](b\n".repeat(n / 6),
    "nested images": "[![a](b)](c".repeat(n / 11),
  };
  for (const [name, src] of Object.entries(inputs)) {
    const start = performance.now();
    for (const block of parseBlocks(src)) if (block.kind === "paragraph" || block.kind === "heading") parseInline(block.text);
    parseInline(src);
    const ms = performance.now() - start;
    assert.ok(ms < 100, `${name}: ${ms.toFixed(0)} ms`);
  }
});

test("a link or image URL the browser can't parse is not one, so rendering never throws", () => {
  for (const bad of ["https://%", "https://[", "https://"]) {
    assert.equal(safeHref(bad), null, bad);
    assert.equal(safeSrc(bad), null, bad);
    assert.ok(!parseInline(`![a](${bad})`).some((p) => p.kind === "image"), bad);
  }
});
