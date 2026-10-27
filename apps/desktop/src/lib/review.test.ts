/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { Agent } from "./api.ts";
import { bracketedPaste, formatReview, isInside, quoteLines, reviewTargets, sendToPane, type QuotableFile, type ReviewComment } from "./review.ts";

const file: QuotableFile = {
  hunks: [{ additionStart: 10, additionCount: 3, additionLineIndex: 0, deletionStart: 10, deletionCount: 2, deletionLineIndex: 0 }],
  additionLines: ["const a = 1;\n", "const b = 2;\n", "return a + b;\n"],
  deletionLines: ["const a = 0;\n", "return a;\n"],
};
const comment = (over: Partial<ReviewComment> = {}): ReviewComment => ({ id: "1", path: "src/a.ts", side: "additions", start: 11, end: 11, quote: ["const b = 2;"], body: "Why 2?", ...over });

test("quotes the lines of each side, in either order, and skips lines the diff doesn't show", () => {
  assert.deepEqual(quoteLines(file, "additions", 10, 11), ["const a = 1;", "const b = 2;"]);
  assert.deepEqual(quoteLines(file, "additions", 12, 11), ["const b = 2;", "return a + b;"]);
  assert.deepEqual(quoteLines(file, "deletions", 11, 11), ["return a;"]);
  assert.deepEqual(quoteLines(file, "additions", 1, 3), []);
  assert.deepEqual(quoteLines(file, "additions", 13, 20), []);
});

test("one comment reads as file:line, the quoted code, then the comment", () => {
  assert.equal(
    formatReview([comment()], "Uncommitted changes"),
    ['Review comments on "Uncommitted changes". Please address each one:', "", "1. src/a.ts:11", "> const b = 2;", "", "Why 2?"].join("\n"),
  );
});

test("several comments are numbered, ranges and removed code are marked", () => {
  const text = formatReview([comment({ start: 10, end: 11, quote: ["const a = 1;", "const b = 2;"] }), comment({ id: "2", side: "deletions", start: 11, end: 11, quote: ["return a;"], body: "Keep it" })]);
  assert.match(text, /^Review comments\. Please address each one:\n\n1\. src\/a\.ts:10-11\n> const a = 1;\n> const b = 2;\n\nWhy 2\?\n\n2\. src\/a\.ts:11 \(removed code\)\n> return a;\n\nKeep it$/);
});

test("a blank quoted line leaves no trailing space", () => {
  assert.match(formatReview([comment({ quote: ["a", "", "b"] })]), /> a\n>\n> b/);
});

test("bracketed paste wraps the text and drops escapes that could end it early", () => {
  assert.equal(bracketedPaste("hi\nthere"), "\x1b[200~hi\nthere\x1b[201~");
  assert.equal(bracketedPaste("a\x1b[201~rm -rf\r\nb\x07"), "\x1b[200~a[201~rm -rf\nb\x1b[201~");
});

test("sending pastes without Enter unless asked, and Enter goes in its own write", async () => {
  const writes: string[] = [];
  const write = async (_id: string, data: string) => void writes.push(data);
  const wait = async () => void writes.push("wait");
  await sendToPane(write, "p", "x", false, wait);
  assert.deepEqual(writes.splice(0), ["\x1b[200~x\x1b[201~"]);
  await sendToPane(write, "p", "x", true, wait);
  assert.deepEqual(writes, ["\x1b[200~x\x1b[201~", "wait", "\r"]);
});

test("isInside matches the folder and what's under it, not a sibling with the same prefix", () => {
  assert.ok(isInside("/r", "/r"));
  assert.ok(isInside("/r/", "/r/sub"));
  assert.ok(!isInside("/r", "/rx"));
});

const agent = (paneId: string, spaceId: string): Agent => ({
  paneId, spaceId, pid: 1, kind: "claude", sessionId: null, name: null, state: "idle", waitingFor: null, args: [], startedAt: 0, stateSince: null, direct: true, summary: null,
});
const spaces = [{ id: "/r", name: "r", path: "/r", branch: null }, { id: "/o", name: "o", path: "/o", branch: null }];
const base = { spaces, agentName: () => "Claude", paneInfo: { a: { command: "claude", cwd: "/r/sub", busy: true }, b: { command: "claude", cwd: "/o", busy: true } } };

test("the only agent inside the diff's folder is preselected, and its siblings come after", () => {
  const r = reviewTargets({ ...base, agents: [agent("b", "/o"), agent("a", "/r")] }, "/r");
  assert.equal(r.preferred, "a");
  assert.deepEqual(r.targets.map((t) => t.paneId), ["a", "b"]);
});

test("two agents in the folder, none, or no folder: nothing is preselected", () => {
  const two = { ...base, agents: [agent("a", "/r"), agent("b", "/r")], paneInfo: { a: { command: "claude", cwd: "/r", busy: true }, b: { command: "claude", cwd: "/r/x", busy: true } } };
  assert.equal(reviewTargets(two, "/r").preferred, null);
  assert.equal(reviewTargets({ ...base, agents: [agent("b", "/o")] }, "/r").preferred, null);
  assert.equal(reviewTargets({ ...base, agents: [agent("a", "/r")] }).preferred, null);
});

test("a plain terminal is never a target: the quoted code would run there on Enter", () => {
  const r = reviewTargets({ ...base, agents: [], paneInfo: { t: { command: "zsh", cwd: "/r", busy: false } } }, "/r");
  assert.deepEqual(r.targets, []);
  assert.equal(r.preferred, null);
});
