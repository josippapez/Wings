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
