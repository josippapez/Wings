import assert from "node:assert/strict";
import test from "node:test";
import { dayTime, pushesOf, timelineOf } from "./pushes.ts";
import { snippetOf } from "./snippet.ts";

const commit = (id, at, comment = `msg ${id}\nbody`) => ({ commitId: id, comment, author: { name: "Ada", date: at } });
const it = (id, createdDate, commits, extra = {}) => ({ id, createdDate, author: { displayName: "Ada" }, commits, sourceRefCommit: { commitId: `head${id}` }, commonRefCommit: { commitId: "base" }, ...extra });

test("iterations become pushes: oldest first, commits newest first with the first line of the message", () => {
  const pushes = pushesOf([
    it(2, "2026-01-02T10:00:00Z", [commit("a", "2026-01-02T09:00:00Z"), commit("b", "2026-01-02T09:30:00Z")]),
    it(1, "2026-01-01T10:00:00Z", [commit("c", "2026-01-01T09:00:00Z")]),
  ]);
  assert.deepEqual(pushes.map((p) => p.id), [1, 2]);
  assert.deepEqual(pushes[1].commits.map((c) => [c.sha, c.message]), [["b", "msg b"], ["a", "msg a"]]);
  assert.equal(pushes[1].head, "head2");
  assert.equal(pushes[1].base, "base");
});

test("a commit shows only in the first push that has it, and a push with nothing new has no card", () => {
  const pushes = pushesOf([
    it(1, "2026-01-01T10:00:00Z", [commit("a", "2026-01-01T09:00:00Z")]),
    it(2, "2026-01-02T10:00:00Z", [commit("a", "2026-01-01T09:00:00Z"), commit("b", "2026-01-02T09:00:00Z")]),
    it(3, "2026-01-03T10:00:00Z", [commit("a", "2026-01-01T09:00:00Z")]),
    it(4, "2026-01-04T10:00:00Z", []),
  ]);
  assert.deepEqual(pushes.map((p) => [p.id, p.commits.map((c) => c.sha)]), [[1, ["a"]], [2, ["b"]]]);
});

test("a cut commit list is marked", () => {
  assert.equal(pushesOf([it(1, "2026-01-01T10:00:00Z", [commit("a", "2026-01-01T09:00:00Z")], { hasMoreCommits: true })])[0].more, true);
});

test("pushes and threads share one timeline, newest first, and pushes are not threads", () => {
  const pushes = pushesOf([it(1, "2026-01-01T10:00:00Z", [commit("a", "2026-01-01T09:00:00Z")]), it(2, "2026-01-03T10:00:00Z", [commit("b", "2026-01-03T09:00:00Z")])]);
  const thread = (id, createdAt, replyAt) => ({ root: { id, createdAt }, replies: replyAt ? [{ id: id + 1, createdAt: replyAt }] : [] });
  const entries = timelineOf([thread(10, "2026-01-02T00:00:00Z"), thread(20, "2026-01-01T00:00:00Z", "2026-01-04T00:00:00Z")], pushes);
  assert.deepEqual(entries.map((e) => (e.kind === "push" ? `push${e.push.id}` : `thread${e.thread.root.id}`)), ["thread20", "push2", "thread10", "push1"]);
  assert.equal(entries.filter((e) => e.kind === "thread").length, 2);
});

test("commit times read as today, yesterday or a date", () => {
  const now = new Date(2026, 9, 14, 15, 0);
  assert.match(dayTime(new Date(2026, 9, 14, 10, 50).toISOString(), now), /^Today at 10:50/);
  assert.match(dayTime(new Date(2026, 9, 13, 9, 33).toISOString(), now), /^Yesterday at 9:33/);
  assert.match(dayTime(new Date(2026, 9, 1, 9, 33).toISOString(), now), /^1 Oct, 9:33/);
});

const file = Array.from({ length: 50 }, (_, i) => `line ${i + 1}`).join("\n") + "\n";

test("a snippet runs 4 lines above to 3 below the commented line", () => {
  const lines = snippetOf(file, 20);
  assert.deepEqual([lines[0].n, lines.at(-1).n], [16, 23]);
  assert.deepEqual(lines.filter((l) => l.hit).map((l) => l.n), [20]);
  assert.equal(lines[4].text, "line 20");
});

test("a snippet marks the whole commented range and stops at the file's edges", () => {
  assert.deepEqual(snippetOf(file, 2, 4).filter((l) => l.hit).map((l) => l.n), [2, 3, 4]);
  assert.equal(snippetOf(file, 2, 4)[0].n, 1);
  assert.equal(snippetOf(file, 49, 50).at(-1).n, 50);
  assert.equal(snippetOf(file, 51).length, 0);
  assert.equal(snippetOf(file, 0).length, 0);
});

test("a long commented range shows its first lines", () => {
  const hits = snippetOf(file, 10, 40).filter((l) => l.hit);
  assert.equal(hits.length, 6);
});
