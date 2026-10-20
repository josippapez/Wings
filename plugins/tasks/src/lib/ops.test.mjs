import test from "node:test";
import assert from "node:assert/strict";
import { lastSession } from "./actor.ts";
import { DESCRIPTION_TEMPLATE, LIMITS, priorityRank } from "./domain.ts";
import { formatIssue } from "./format.ts";
import { applyOp } from "./ops.ts";
import { boardColumns } from "./orderings.ts";
import { create, fresh } from "./testing.mjs";

test("keys count up per project and are never reused after a delete", () => {
  let s = fresh();
  s = create(s, "a").store;
  s = create(s, "b").store;
  assert.deepEqual(s.issues.map((i) => i.key), ["WEB-1", "WEB-2"]);
  s = applyOp(s, { kind: "remove", key: "web-2" }).store;
  s = create(s, "c").store;
  assert.deepEqual(s.issues.map((i) => i.key), ["WEB-1", "WEB-3"]);
  s = applyOp(s, { kind: "remove", key: "WEB-3" }).store;
  assert.equal(create(s, "d").issue.key, "WEB-4");
});

test("the log is append-only: status and link changes log themselves and earlier entries stay as they were", () => {
  let s = create(fresh(), "A", {}, "you", 1).store;
  const before = s.issues[0].log;
  s = applyOp(s, { kind: "update", key: "WEB-1", status: "in_progress", branch: "feat/a", pr: "https://example.com/p/1", commit: "ABCDEF1234" }, "you", 2).store;
  s = applyOp(s, { kind: "note", key: "WEB-1", text: " did it " }, "you", 3).store;
  const log = s.issues[0].log;
  assert.deepEqual(log.map((e) => [e.kind, e.text]), [["created", "Created"], ["status", "To do → In progress"], ["link", "Branch: feat/a"], ["link", "PR: https://example.com/p/1"], ["link", "Commit: abcdef1"], ["note", "did it"]]);
  assert.deepEqual(log.slice(0, before.length), before);
  assert.equal(before.length, 1);
  // Nothing in the operations edits or removes an entry, and the input store isn't mutated.
  assert.equal(Object.isFrozen(log), false);
  const again = applyOp(s, { kind: "update", key: "WEB-1", status: "in_progress", branch: "feat/a", commit: "abcdef1" }, "you", 4);
  assert.equal(again.changed, false);
  assert.equal(again.store, s);
  const cleared = applyOp(s, { kind: "update", key: "WEB-1", branch: "" }, "you", 5).store.issues[0];
  assert.equal(cleared.log.at(-1).text, "Branch removed");
  assert.equal(cleared.links.branch, undefined);
  assert.equal(cleared.log.length, log.length + 1);
});

test("a full log refuses more entries instead of dropping old ones", () => {
  let s = create(fresh(), "A").store;
  for (let i = 1; i < LIMITS.log; i++) s = applyOp(s, { kind: "note", key: "WEB-1", text: `n${i}` }).store;
  assert.equal(s.issues[0].log.length, LIMITS.log);
  assert.throws(() => applyOp(s, { kind: "note", key: "WEB-1", text: "more" }), /already has/);
});

test("agent entries are stamped with the session and the issue records it once, refreshed on later calls", () => {
  const sess = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "first name" };
  let s = create(fresh(), "A", {}, sess, 10).store;
  assert.deepEqual(s.issues[0].log[0].by, sess);
  assert.deepEqual(s.issues[0].sessions, [{ id: sess.id, name: "first name", seen: 10 }]);
  const before = s;
  s = applyOp(s, { kind: "note", key: "WEB-1", text: "x" }, { id: sess.id, name: "renamed" }, 20).store;
  assert.deepEqual(s.issues[0].sessions, [{ id: sess.id, name: "renamed", seen: 20 }]);
  assert.equal(before.issues[0].sessions[0].seen, 10, "the old store is left alone");
  s = applyOp(s, { kind: "note", key: "WEB-1", text: "y" }, { id: "other" }, 30).store;
  assert.deepEqual(s.issues[0].sessions.map((x) => x.id), [sess.id, "other"]);
  assert.equal(lastSession(s.issues[0]).id, "other");
  // The user and an agent without a session id don't become sessions.
  s = applyOp(s, { kind: "note", key: "WEB-1", text: "z" }, "you", 40).store;
  s = applyOp(s, { kind: "note", key: "WEB-1", text: "w" }, { id: "" }, 50).store;
  assert.equal(s.issues[0].sessions.length, 2);
});

test("an issue keeps sessions of different agents side by side, each with its kind", () => {
  let s = create(fresh(), "A", {}, { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "claude one", kind: "claude" }, 10).store;
  s = applyOp(s, { kind: "note", key: "WEB-1", text: "x" }, { id: "ses_abc12345", name: "oc one", kind: "opencode" }, 20).store;
  assert.deepEqual(s.issues[0].sessions, [
    { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "claude one", kind: "claude", seen: 10 },
    { id: "ses_abc12345", name: "oc one", kind: "opencode", seen: 20 },
  ]);
  assert.equal(lastSession(s.issues[0]).kind, "opencode");
});

test("unknown keys, bad status and bad links are errors that say what is wrong", () => {
  const s = create(fresh(), "A").store;
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-9", status: "done" }), /No issue WEB-9/);
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-1", status: "later" }), /status must be one of/);
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-1", pr: "http://x.example" }), /https URL/);
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-1", commit: "xyz" }), /commit must be/);
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-1", branch: "a b" }), /spaces/);
  assert.throws(() => create(s, "   "), /required/);
});

test("priority: set on create, changed and cleared with a log entry, refused when unknown, and sorts urgent first", () => {
  let s = create(fresh(), "Fix it", { priority: "high" }).store;
  assert.equal(s.issues[0].priority, "high");
  assert.equal(create(fresh(), "Plain").store.issues[0].priority, undefined);
  assert.equal(create(fresh(), "None", { priority: "none" }).store.issues[0].priority, undefined);
  s = applyOp(s, { kind: "update", key: "WEB-1", priority: "urgent" }, "you", 2).store;
  assert.equal(s.issues[0].priority, "urgent");
  assert.equal(s.issues[0].log.at(-1).text, "Priority: High → Urgent");
  s = applyOp(s, { kind: "update", key: "WEB-1", priority: "none" }, "you", 3).store;
  assert.equal("priority" in s.issues[0], false);
  assert.equal(applyOp(s, { kind: "update", key: "WEB-1", priority: "none" }).changed, false);
  assert.throws(() => applyOp(s, { kind: "update", key: "WEB-1", priority: "p1" }), /priority must be one of/);
  const mk = (n, priority) => ({ key: `WEB-${n}`, title: `t${n}`, type: "task", status: "todo", priority, sessions: [], log: [], createdAt: 0, updatedAt: 0 });
  const col = boardColumns([mk(1), mk(2, "low"), mk(3, "urgent"), mk(4, "high")])[0];
  assert.deepEqual(col.issues.map((i) => i.key), ["WEB-3", "WEB-4", "WEB-2", "WEB-1"]);
  assert.ok(priorityRank({}) > priorityRank({ priority: "low" }));
});

test("an untouched template is stored as no description, and get says to fill it", () => {
  const describe = (s, description) => applyOp(s, { kind: "update", key: "WEB-1", description }).store;
  let s = create(fresh(), "a", { description: DESCRIPTION_TEMPLATE }).store;
  assert.equal(s.issues[0].description, undefined);
  s = describe(s, "## Goal\nx");
  assert.equal(s.issues[0].description, "## Goal\nx");
  s = describe(s, DESCRIPTION_TEMPLATE);
  assert.equal(s.issues[0].description, undefined);
  assert.match(formatIssue(s.issues[0]), /No description yet; fill Goal, Scope, Done when, Validation/);
});

test("files: one row per path with the newest description, time and session; past 100 the oldest go; texts are cut; no log entry", () => {
  const a = { id: "s-a", name: "one", kind: "claude" };
  const b = { id: "s-b" };
  let s = create(fresh(), "A", {}, "you", 1).store;
  s = applyOp(s, { kind: "files", key: "WEB-1", files: [{ path: "x.ts", what: "first" }, { path: "y.ts", what: "why" }] }, a, 10).store;
  s = applyOp(s, { kind: "files", key: "WEB-1", files: [{ path: "x.ts", what: "second" }, { path: "x.ts", what: "third" }] }, b, 20).store;
  const issue = s.issues[0];
  assert.deepEqual(issue.files, [{ path: "x.ts", what: "third", at: 20, session: { id: "s-b" } }, { path: "y.ts", what: "why", at: 10, session: { id: "s-a", name: "one", kind: "claude" } }]);
  assert.equal(issue.log.length, 1, "no log entry");
  assert.deepEqual(issue.sessions.map((x) => [x.id, x.seen]), [["s-a", 10], ["s-b", 20]], "the session is recorded and refreshed like any write");
  assert.equal(applyOp(s, { kind: "files", key: "WEB-1", files: [] }, a, 30).changed, false);

  const many = Array.from({ length: 105 }, (_, i) => ({ path: `f${i}.ts`, what: "w" }));
  let t = applyOp(create(fresh(), "A").store, { kind: "files", key: "WEB-1", files: many.slice(0, 60) }, a, 1).store;
  t = applyOp(t, { kind: "files", key: "WEB-1", files: many.slice(60) }, a, 2).store;
  const files = t.issues[0].files;
  assert.equal(files.length, LIMITS.files);
  assert.equal(files.length, 100);
  assert.deepEqual(files.slice(0, 45).map((f) => f.at), Array(45).fill(2));
  assert.equal(files.filter((f) => f.at === 1).length, 55);

  const long = applyOp(create(fresh(), "A").store, { kind: "files", key: "WEB-1", files: [{ path: "p".repeat(500), what: "w".repeat(500) }] }, a, 1).store.issues[0].files[0];
  assert.equal(long.path.length, LIMITS.filePath);
  assert.equal(long.what.length, LIMITS.fileWhat);
});

test("an issue with a full log still takes its files, and an old issue without files reads and formats fine", () => {
  let s = create(fresh(), "A").store;
  for (let i = 1; i < LIMITS.log; i++) s = applyOp(s, { kind: "note", key: "WEB-1", text: `n${i}` }).store;
  assert.equal(s.issues[0].log.length, LIMITS.log);
  const out = applyOp(s, { kind: "files", key: "WEB-1", files: [{ path: "a.ts", what: "x" }] }, { id: "s" }, 5);
  assert.equal(out.changed, true);
  assert.equal(out.issue.files.length, 1);
  assert.equal(out.issue.log.length, LIMITS.log);
  const old = create(fresh(), "Old").store.issues[0];
  assert.equal(old.files, undefined);
  assert.doesNotMatch(formatIssue(old), /Files changed/);
});

test("the get text lists the newest files first, at most 30, then how many more", () => {
  const files = Array.from({ length: 35 }, (_, i) => ({ path: `f${i}.ts`, what: i ? "w" : "", at: 1000 + i, session: i % 2 ? { id: "s", name: "agent" } : { id: "t", kind: "opencode" } }));
  const text = formatIssue({ ...create(fresh(), "A").store.issues[0], files });
  const section = text.split("\n\nSessions:")[0].split("\n\n").at(-1).split("\n");
  assert.equal(section[0], "Files changed (35):");
  assert.match(section[1], /^ {2}f34\.ts — w \(OpenCode, 1970-01-01T00:00/);
  assert.match(section[2], /^ {2}f33\.ts — w \(agent, /);
  assert.match(section[30], /^ {2}f5\.ts/);
  assert.equal(section[31], "  and 5 more");
  assert.match(formatIssue({ ...create(fresh(), "A").store.issues[0], files: [files[0]] }), /f0\.ts \(OpenCode, /);
});
