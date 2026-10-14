import test from "node:test";
import assert from "node:assert/strict";
import { agentCommand, applyOp, boardColumns, BUDGET, estimateBytes, LIMITS, lastSession, migrateTasks, openIssues, PLAIN_WORD, prefixFor, resolvePane, resumeCommand } from "./issues.ts";
import { formatIssue, parseTool } from "./tools.ts";

const fresh = (name = "web") => migrateTasks([], name);
const create = (store, title, extra = {}, by = "you", now = 1) => applyOp(store, { kind: "create", title, ...extra }, by, now);

test("migration: tasks become issues, oldest first, notes and doing mapped, branch kept", () => {
  const store = migrateTasks(
    [
      { id: "b", title: "Second", notes: "n", status: "doing", links: { branch: "feat/x" }, createdAt: 20, updatedAt: 30 },
      { id: "a", title: "First", status: "todo", createdAt: 10, updatedAt: 10 },
      { id: "c", title: "Third", status: "done", createdAt: 30, updatedAt: 40 },
      { junk: true },
    ],
    "Web Shop",
  );
  assert.equal(store.v, 3);
  assert.equal(store.prefix, "WEB");
  assert.equal(store.next, 4);
  assert.deepEqual(store.issues.map((i) => [i.key, i.title, i.status]), [["WEB-1", "First", "todo"], ["WEB-2", "Second", "in_progress"], ["WEB-3", "Third", "done"]]);
  const second = store.issues[1];
  assert.equal(second.description, "n");
  assert.deepEqual(second.links, { branch: "feat/x" });
  assert.equal(second.updatedAt, 30);
  assert.deepEqual(second.log, [{ at: 20, by: "you", kind: "created", text: "Imported from Tasks" }]);
  assert.deepEqual(migrateTasks(null, "x").issues, []);
});

test("key prefix: letters only, up to three, uppercase, ISS when none", () => {
  assert.equal(prefixFor("web"), "WEB");
  assert.equal(prefixFor("my-app-2"), "MYA");
  assert.equal(prefixFor("42-x"), "X");
  assert.equal(prefixFor("12345"), "ISS");
  assert.equal(prefixFor("Ünï"), "N");
  assert.equal(prefixFor(""), "ISS");
});

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

test("resolvePane maps a tool call's paneId to its project and Claude session", () => {
  const panes = [
    { paneId: "p1", project: "web", projectId: "proj1", session: { sessionId: "s-1", name: "fix totals" } },
    { paneId: "p2", project: "api", projectId: "proj2", session: { sessionId: "s-2", name: null } },
    { paneId: "p3", project: "api", projectId: "proj2", session: null },
  ];
  assert.deepEqual(resolvePane(panes, "p1"), { projectId: "proj1", projectName: "web", actor: { id: "s-1", name: "fix totals" } });
  assert.deepEqual(resolvePane(panes, "p2").actor, { id: "s-2" });
  assert.deepEqual(resolvePane(panes, "p3").actor, { id: "" });
  assert.throws(() => resolvePane(panes, "nope"), /Can't tell which project/);
  assert.throws(() => resolvePane(panes, null), /Can't tell which project/);
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

test("tool input validation bounds every field and rejects strays", () => {
  assert.deepEqual(parseTool("get", { key: " web-3 " }), { tool: "get", key: "WEB-3" });
  assert.throws(() => parseTool("get", {}), /key is required/);
  assert.throws(() => parseTool("get", { key: "A", extra: 1 }), /Unknown field: extra/);
  assert.throws(() => parseTool("create", { title: "x".repeat(LIMITS.title + 1) }), /title is limited to 200/);
  assert.throws(() => parseTool("create", { title: "ok", description: "x".repeat(LIMITS.description + 1) }), /description is limited to 20,000/);
  assert.throws(() => parseTool("log", { key: "WEB-1", text: "x".repeat(LIMITS.note + 1) }), /text is limited to 4,000/);
  assert.throws(() => parseTool("log", { key: "WEB-1", text: "  " }), /text is required/);
  assert.throws(() => parseTool("create", { title: 5 }), /title must be text/);
  assert.throws(() => parseTool("create", { title: "t", status: "wip" }), /status must be one of/);
  assert.throws(() => parseTool("update", { key: "WEB-1" }), /at least one of/);
  assert.throws(() => parseTool("list", { status: "x" }), /status must be one of/);
  assert.throws(() => parseTool("get", null), /must be an object/);
  assert.deepEqual(parseTool("update", { key: "web-1", status: "done", commit: "abc1234" }).op, { kind: "update", key: "WEB-1", title: undefined, description: undefined, status: "done", branch: undefined, pr: undefined, commit: "abc1234", type: undefined, parent: undefined, relatedAdd: undefined, relatedRemove: undefined, blockedByAdd: undefined, blockedByRemove: undefined });
});

test("a tool-created issue gets the agent's session, and get shows description, links, sessions and log", () => {
  const sess = { id: "sess-1", name: "agent one" };
  const { op } = parseTool("create", { title: "Do it", description: "Goal", status: "in_progress" });
  let s = applyOp(fresh(), op, sess, 5).store;
  s = applyOp(s, parseTool("log", { key: "WEB-1", text: "next: tests" }).op, sess, 6).store;
  const text = formatIssue(s.issues[0]);
  for (const part of ["WEB-1 [in_progress] (task) Do it", "Goal", "sess-1", "agent one", "agent one note: next: tests"]) assert.ok(text.includes(part), part);
});

test("board columns and the open list: to do by key, others by activity, canceled only on request", () => {
  const mk = (n, status, updatedAt) => ({ key: `WEB-${n}`, title: `t${n}`, status, sessions: [], log: [], createdAt: 0, updatedAt });
  const issues = [mk(2, "todo", 5), mk(1, "todo", 9), mk(3, "in_progress", 1), mk(4, "in_progress", 8), mk(5, "done", 2), mk(6, "canceled", 3), mk(7, "in_review", 3)];
  const cols = boardColumns(issues);
  assert.deepEqual(cols.map((c) => c.status), ["todo", "in_progress", "in_review", "done"]);
  assert.deepEqual(cols.map((c) => c.issues.map((i) => i.key)), [["WEB-1", "WEB-2"], ["WEB-4", "WEB-3"], ["WEB-7"], ["WEB-5"]]);
  assert.equal(boardColumns(issues, true).at(-1).status, "canceled");
  assert.deepEqual(openIssues(issues).map((i) => i.key), ["WEB-3", "WEB-4", "WEB-7", "WEB-1", "WEB-2"]);
});

test("resume and start commands are plain words Wings accepts, and refuse anything else", () => {
  const id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
  assert.equal(resumeCommand(id), `claude --resume ${id}`);
  for (const word of resumeCommand(id).split(" ")) assert.match(word, PLAIN_WORD);
  // The prompt goes as one openPane arg, a sentence Wings quotes; it holds nothing a shell or Claude reads as a flag.
  const start = agentCommand("WEB-12");
  assert.equal(start.command, "claude");
  assert.equal(start.args.length, 1);
  assert.match(start.args[0], /^Continue issue WEB-12\. Read it with tasks__get/);
  assert.ok(!start.args[0].startsWith("-") && !/[\x00-\x1f'`$]/.test(start.args[0]));
  for (const bad of ["x; rm -rf ~", "$(id)", "a b", "", "../x", "id'quote"]) assert.throws(() => resumeCommand(bad), /session id/);
  for (const bad of ["WEB-1; ls", "web-1", "WEB", "WEB-1 x", "$(id)-1", "WEBS-1"]) assert.throws(() => agentCommand(bad), /issue key/);
});

test("size: 200 issues with 10 log entries each fit well inside the budget, and the estimate counts UTF-8 bytes", () => {
  let s = fresh();
  const sess = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "checkout summary" };
  for (let i = 0; i < 200; i++) {
    s = create(s, `Issue number ${i} with a title of a normal length`, { description: "Goal: do the thing.\n\nScope: this and that.\n\nDone when: it works and has a test." }, "you", i).store;
    for (let j = 0; j < 9; j++) s = applyOp(s, { kind: "note", key: `WEB-${i + 1}`, text: "Did the next step of the work and checked that it still passes. Next: the follow-up." }, sess, i + j).store;
  }
  const bytes = estimateBytes(s);
  console.log(`# 200 issues x 10 log entries: ${bytes} bytes (${(bytes / 1000).toFixed(0)} KB, ${(bytes / 200).toFixed(0)} bytes per issue)`);
  assert.ok(bytes < BUDGET, "inside the per-project budget");
  assert.ok(bytes < 1_000_000 / 2, "leaves room for other projects in the 1 MB");
  assert.equal(estimateBytes({ ...fresh(), prefix: "é" }), new TextEncoder().encode(JSON.stringify({ ...fresh(), prefix: "é" })).length);
});
