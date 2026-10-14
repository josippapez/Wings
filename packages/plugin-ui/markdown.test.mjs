// node --test: the Markdown parser and the component's markup.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement as h } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { parseBlocks, parseInline, safeHref, safeSrc, MAX_DATA_IMAGE } from "./markdown.js";
import { Markdown } from "./ui.js";

const html = (source, props) => renderToStaticMarkup(h(Markdown, { source, ...props }));

test("a GFM table has a head, alignment and padded rows", () => {
  const [t] = parseBlocks("| Name | Port | Note |\n| :--- | ---: | :-: |\n| dev | 5173 |\n| a \\| b | 1 | 2 | 3 |");
  assert.deepEqual(t, { kind: "table", align: ["left", "right", "center"], head: ["Name", "Port", "Note"], rows: [["dev", "5173", ""], ["a | b", "1", "2"]] });
  assert.match(html("| A | B |\n|---|---|\n| 1 | **2** |"), /<table[^>]*><thead><tr><th scope="col">A<\/th><th scope="col">B<\/th><\/tr><\/thead><tbody><tr><td>1<\/td><td><strong>2<\/strong><\/td>/);
});

test("a pipe line without a delimiter row is a paragraph", () => {
  assert.deepEqual(parseBlocks("a | b\nc | d").map((b) => b.kind), ["paragraph"]);
});

test("task items and depth", () => {
  const [l] = parseBlocks("- [x] done\n- [ ] todo\n  - nested\n- plain");
  assert.deepEqual(l.items.map((i) => [i.text, i.depth, i.checked]), [["done", 0, true], ["todo", 0, false], ["nested", 1, null], ["plain", 0, null]]);
  assert.match(html("- [x] done"), /<input(?=[^>]*type="checkbox")(?=[^>]*disabled="")[^>]*checked=""/);
});

test("headings, code fences, quotes and rules", () => {
  const kinds = parseBlocks("# T\n\n```js\n<b>x</b>\n```\n> q\n---\ntext").map((b) => b.kind);
  assert.deepEqual(kinds, ["heading", "code", "quote", "rule", "paragraph"]);
  assert.match(html("```\n<script>x</script>\n```"), /<pre[^>]*><code>&lt;script&gt;x&lt;\/script&gt;<\/code><\/pre>/);
});

test("HTML is never rendered, only escaped", () => {
  const out = html('<img src=x onerror=alert(1)> and <script>alert(1)</script>\n\n| <b>a</b> | b |\n|---|---|\n| <i>c</i> | d |');
  assert.doesNotMatch(out, /<script|<img|<b>|<i>/);
  assert.match(out, /&lt;script&gt;/);
});

test("only https links are links", () => {
  for (const bad of ["javascript:alert(1)", "http://example.com", "data:text/html,x", "//example.com", "/relative", "JaVaScRiPt:alert(1)", "https://x.com/a\"onclick=1"]) {
    assert.equal(safeHref(bad), null, bad);
    assert.doesNotMatch(html(`[x](${bad})`), /<a /, bad);
  }
  assert.match(html("[x](https://example.com/a?b=1)"), /<a [^>]*href="https:\/\/example.com\/a\?b=1"/);
});

test("bare https URLs become links without the trailing full stop", () => {
  assert.deepEqual(parseInline("see https://example.com/x."), [{ kind: "text", text: "see " }, { kind: "link", text: "https://example.com/x", href: "https://example.com/x" }, { kind: "text", text: "." }]);
});

test("images: https and small raster data URLs only", () => {
  assert.match(html("![logo](https://example.com/a.png)"), /Load image from example.com/);
  const png = "data:image/png;base64,iVBORw0KGgo=";
  assert.equal(safeSrc(png), png);
  for (const bad of ["http://example.com/a.png", "javascript:alert(1)", "data:image/svg+xml;base64,PHN2Zz4=", "data:text/html;base64,PGI+", "data:image/png;base64," + "A".repeat(MAX_DATA_IMAGE), "file:///etc/passwd", "/a.png"]) {
    assert.equal(safeSrc(bad), null, bad.slice(0, 30));
    assert.doesNotMatch(html(`![alt text](${bad})`), /<img/);
    assert.match(html(`![alt text](${bad})`), /alt text/);
  }
});

test("inline renders a span with only inline parts", () => {
  assert.match(html("**a** `b`", { inline: true }), /^<span class="w-md"><strong>a<\/strong> <code[^>]*>b<\/code><\/span>$/);
});

test("pathological 40k inputs parse in under 100 ms", () => {
  const n = 40_000;
  const inputs = {
    "open brackets": "[".repeat(n),
    "open image brackets": "![".repeat(n / 2),
    "bracket then no paren": "[" + "a".repeat(n),
    "link text without close": "[a](" + "b".repeat(n),
    "dots after a url": "https://a.b/" + ".".repeat(n),
    "dots then x": "https://a.b/" + "!".repeat(n) + "x",
    "heading spaces": "# a" + " ".repeat(n) + "x",
    "heading hashes": "# a " + "#".repeat(n) + " x",
    "backticks": "`a ".repeat(n / 3),
    "stars": "*a ".repeat(n / 3),
    "double stars": "**a ".repeat(n / 4),
    "tildes": "~~a ".repeat(n / 4),
    "rule-ish": "- ".repeat(n / 2) + "x",
    "table pipes": "|".repeat(n) + "\n|" + "-|".repeat(n / 2),
    "many lines of brackets": ("[a](b\n").repeat(n / 6),
    "colons": ":".repeat(n),
    "open shortcodes": ":a".repeat(n / 2),
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

test("a linked image is a link whose text is the image's alt, never a link to the image", () => {
  assert.deepEqual(parseInline("[![logo](https://img.example/a.png)](https://example.com/x)"), [{ kind: "link", text: "logo", href: "https://example.com/x" }]);
  assert.deepEqual(parseInline("[![logo](https://img.example/a.png)](javascript:alert1)"), [{ kind: "text", text: "logo" }]);
  assert.deepEqual(parseInline("[![](https://img.example/a.png)](https://example.com/x)"), [{ kind: "link", text: "https://example.com/x", href: "https://example.com/x" }]);
});

test("a remote https image renders as a button that loads it on click, a data image renders at once", () => {
  const remote = html("![secret](https://attacker.example/?d=1)");
  assert.doesNotMatch(remote, /<img|attacker.example\/\?d=1/);
  assert.match(remote, /<button[^>]*class="w-md-image-load"[^>]*>Load image from attacker.example<\/button>/);
  assert.match(html("![dot](data:image/png;base64,iVBORw0KGgo=)"), /<img[^>]*src="data:image\/png/);
});

test("link reference definitions are hidden, as Markdown hides them", () => {
  const blocks = parseBlocks("A finding.\n\n[ISSUE_KEY]: AbC-1\n[PROJECT_KEY]: acme_web\n\nNext");
  assert.deepEqual(blocks.map((b) => b.text), ["A finding.", "Next"]);
  // A task item or a link at the start of a line isn't one.
  assert.equal(parseBlocks("- [x] done")[0].kind, "list");
  assert.equal(parseBlocks("[docs](https://example.com): read them")[0].kind, "paragraph");
});

test("<details> folds its blocks under its <summary>, and nothing of the HTML shows", () => {
  const src = "Logs the raw error.\n<details>\n<summary>Context & sources</summary>\n\n**Why:** it leaks.\n\n- docs/a.md\n</details>\n\nAfter";
  const blocks = parseBlocks(src);
  assert.deepEqual(blocks.map((b) => b.kind), ["paragraph", "details", "paragraph"]);
  assert.equal(blocks[1].summary, "Context & sources");
  assert.deepEqual(blocks[1].blocks.map((b) => b.kind), ["paragraph", "list"]);
  const out = html(src);
  assert.match(out, /<details class="w-md-details"><summary class="w-md-summary">Context &amp; sources<\/summary>/);
  assert.doesNotMatch(out, /&lt;\/?(details|summary)/);
  // The tag can follow text on the same line, as some bots write it; inside inline code it stays text.
  const sameLine = parseBlocks("Logs the raw error. <details> <summary>Context</summary>\n\n**Why:** it leaks.\n\n</details>");
  assert.deepEqual(sameLine.map((b) => b.kind), ["paragraph", "details"]);
  assert.equal(sameLine[0].text, "Logs the raw error.");
  assert.equal(sameLine[1].summary, "Context");
  assert.equal(parseBlocks("Use `<details>` for folds")[0].kind, "paragraph");
  // Without a summary it reads "Details"; one left open runs to the end; folds nest.
  assert.equal(parseBlocks("<details>\ntext")[0].summary, "Details");
  const nested = parseBlocks("<details><summary>a</summary>\n<details><summary>b</summary>\nin\n</details>\nout\n</details>");
  assert.deepEqual(nested[0].blocks.map((b) => b.kind), ["details", "paragraph"]);
  assert.equal(nested[0].blocks[0].blocks[0].text, "in");
});
