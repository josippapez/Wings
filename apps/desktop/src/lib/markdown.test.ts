/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { parseBlocks, parseInline, safeHref } from "./markdown.ts";

test("headings, paragraphs, lists and fenced code come out as blocks", () => {
  assert.deepEqual(parseBlocks("# Title\nsome\ntext\n\n- a\n- b\n\n1. one\n\n```sh\nls <b>\n```"), [
    { kind: "heading", level: 1, text: "Title" },
    { kind: "paragraph", text: "some text" },
    { kind: "list", ordered: false, items: ["a", "b"] },
    { kind: "list", ordered: true, items: ["one"] },
    { kind: "code", text: "ls <b>" },
  ]);
});

test("only https links stay links; images show their alt text", () => {
  assert.equal(safeHref("javascript:alert(1)"), null);
  assert.equal(safeHref("http://example.com"), null);
  assert.equal(safeHref("https://example.com/a"), "https://example.com/a");
  assert.deepEqual(parseInline("see [docs](https://x.dev) and [bad](javascript:x) ![logo](https://x.dev/a.png) `ls`"), [
    { kind: "text", text: "see " },
    { kind: "link", text: "docs", href: "https://x.dev" },
    { kind: "text", text: " and " },
    { kind: "text", text: "bad" },
    { kind: "text", text: " " },
    { kind: "text", text: "logo" },
    { kind: "text", text: " " },
    { kind: "code", text: "ls" },
  ]);
});
