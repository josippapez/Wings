import assert from "node:assert/strict";
import test from "node:test";
import { ago, focusOf, isAutomated, kindOf, oneLine, threadsOf } from "./threads.ts";

const c = (id, replyTo, createdAt, extra = {}) => ({ id, replyTo, path: null, line: null, side: "additions", author: "Ada", body: "x", createdAt, url: "", ...extra });

test("replies go under their first comment and read oldest first; the thread with the newest comment comes first", () => {
  const threads = threadsOf([
    c(2, null, "2026-01-02T00:00:00Z"),
    c(3, 1, "2026-01-03T00:00:00Z"),
    c(1, null, "2026-01-01T00:00:00Z"),
    c(4, 1, "2026-01-02T12:00:00Z"),
    c(5, null, "2026-01-02T18:00:00Z"),
  ]);
  // Thread 1 started first but its reply on the 3rd is the newest comment, so a reply brings a thread back to the top.
  assert.deepEqual(threads.map((t) => [t.root.id, t.replies.map((r) => r.id)]), [[1, [4, 3]], [5, []], [2, []]]);
});

test("a thread is resolved, automated or open", () => {
  const t = (extra) => ({ root: c(1, null, "2026-01-01T00:00:00Z", extra), replies: [] });
  assert.equal(kindOf(t({})), "open");
  assert.equal(kindOf(t({ thread: { state: "active", label: "Active" } })), "open");
  assert.equal(kindOf(t({ thread: { state: "resolved", label: "Resolved" } })), "resolved");
  assert.equal(kindOf(t({ system: true })), "automated");
  assert.equal(kindOf(t({ author: "release-helper[bot]" })), "automated");
  assert.equal(kindOf(t({ author: "Project Collection Build Service (Acme)" })), "automated");
  // Resolved wins over automated, so a resolved bot thread is a resolved thread.
  assert.equal(kindOf(t({ author: "x[bot]", thread: { state: "resolved", label: "Resolved" } })), "resolved");
  assert.equal(isAutomated(c(1, null, "", { author: "Abbott" })), false);
});

test("a collapsed thread's summary is one plain line", () => {
  assert.equal(oneLine("**Bold** and `code`\n\n- [link](https://example.com)"), "Bold and code - link");
  assert.equal(oneLine("```js\nlong\n```\nafter"), "code after");
  assert.equal(oneLine("## Report\n\n| File | Lines |\n|---|---|\n| a.ts | 9% |"), "Report File Lines a.ts 9%");
  assert.equal(oneLine("a".repeat(200), 10), "aaaaaaaaa…");
});

test("relative times", () => {
  const now = Date.parse("2026-03-01T12:00:00Z");
  assert.equal(ago("2026-03-01T11:59:50Z", now), "just now");
  assert.equal(ago("2026-03-01T11:55:00Z", now), "5 min ago");
  assert.equal(ago("2026-03-01T09:00:00Z", now), "3 h ago");
  assert.equal(ago("2026-02-27T12:00:00Z", now), "2 d ago");
  assert.equal(ago("nonsense", now), "");
});

test("View in diff points at the thread's file and the line on its side", () => {
  assert.deepEqual(focusOf(c(1, null, "t", { path: "src/a.ts", line: 12 })), { path: "src/a.ts", line: 12, side: "new" });
  assert.deepEqual(focusOf(c(1, null, "t", { path: "src/a.ts", line: 3, side: "deletions" })), { path: "src/a.ts", line: 3, side: "old" });
  assert.deepEqual(focusOf(c(1, null, "t", { path: "src/a.ts", line: null })), { path: "src/a.ts" }, "code that changed since has no line");
  assert.equal(focusOf(c(1, null, "t")), undefined, "a comment on the whole PR has no file");
});
