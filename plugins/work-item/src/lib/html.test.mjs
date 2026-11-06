import assert from "node:assert/strict";
import test from "node:test";
import { blocksOf, parseHtml } from "./html.ts";

const lines = (html) => blocksOf(parseHtml(html)).map((b) => ({ kind: b.kind, depth: b.depth, marker: b.marker, text: b.spans.map((s) => s.text).join("") }));

test("paragraphs, nested lists and code become lines, and entities are decoded", () => {
  assert.deepEqual(lines("<p>Fails on <code>503</code> &amp; retries</p><ol><li>Start<ul><li>Stop the API</li></ul></li></ol><pre><code>a\n  b</code></pre>"), [
    { kind: "text", depth: 0, marker: undefined, text: "Fails on 503 & retries" },
    { kind: "item", depth: 1, marker: "1.", text: "Start" },
    { kind: "item", depth: 2, marker: "•", text: "Stop the API" },
    { kind: "code", depth: 0, marker: undefined, text: "a\n  b" },
  ]);
});

test("scripts and styles are dropped, and only an Azure DevOps link is kept as one", () => {
  const spans = blocksOf(parseHtml('<script>alert(1)</script><p><a href="https://dev.azure.com/acme/x">here</a> <a href="https://example.com">there</a></p>')).flatMap((b) => b.spans);
  assert.deepEqual(spans.map((s) => [s.text, s.href]), [["here", "https://dev.azure.com/acme/x"], [" ", undefined], ["there", undefined]]);
});
