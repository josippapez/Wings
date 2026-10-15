import test from "node:test";
import assert from "node:assert/strict";
import { boardColumns, boardLanes, issueTree, openIssues, trailTo } from "./orderings.ts";

test("board columns and the open list: to do by key, others by activity, canceled only on request", () => {
  const mk = (n, status, updatedAt) => ({ key: `WEB-${n}`, title: `t${n}`, status, sessions: [], log: [], createdAt: 0, updatedAt });
  const issues = [mk(2, "todo", 5), mk(1, "todo", 9), mk(3, "in_progress", 1), mk(4, "in_progress", 8), mk(5, "done", 2), mk(6, "canceled", 3), mk(7, "in_review", 3)];
  const cols = boardColumns(issues);
  assert.deepEqual(cols.map((c) => c.status), ["todo", "in_progress", "in_review", "done"]);
  assert.deepEqual(cols.map((c) => c.issues.map((i) => i.key)), [["WEB-1", "WEB-2"], ["WEB-4", "WEB-3"], ["WEB-7"], ["WEB-5"]]);
  assert.equal(boardColumns(issues, true).at(-1).status, "canceled");
  assert.deepEqual(openIssues(issues).map((i) => i.key), ["WEB-3", "WEB-4", "WEB-7", "WEB-1", "WEB-2"]);
});

test("board lanes: one row per parent, open parents first, each issue once, no-parent row last", () => {
  const mk = (n, status, extra = {}) => ({ key: `WEB-${n}`, title: `t${n}`, type: "task", status, sessions: [], log: [], createdAt: 0, updatedAt: n, ...extra });
  const issues = [
    mk(1, "done", { type: "epic" }),
    mk(2, "in_progress", { type: "epic" }),
    mk(3, "todo", { parent: "WEB-1" }),
    mk(4, "in_review", { parent: "WEB-2", type: "feature" }),
    mk(5, "todo", { parent: "WEB-4" }),
    mk(6, "canceled", { parent: "WEB-2" }),
    mk(7, "todo", { parent: "WEB-99" }),
    mk(8, "canceled", { type: "feature" }),
    mk(9, "todo", { parent: "WEB-8" }),
  ];
  const lanes = boardLanes(issues);
  // A canceled parent still heads its children's row; a parent that isn't in the project leaves its child with no parent.
  assert.deepEqual(lanes.map((l) => l.parent?.key ?? null), ["WEB-2", "WEB-4", "WEB-1", "WEB-8", null]);
  const keys = (l) => l.columns.flatMap((c) => c.issues.map((i) => i.key));
  assert.deepEqual(lanes.map(keys), [["WEB-4"], ["WEB-5"], ["WEB-3"], ["WEB-9"], ["WEB-7", "WEB-2", "WEB-1"]]);
  assert.deepEqual(lanes.map((l) => l.count), [1, 1, 1, 1, 3]);
  assert.deepEqual(lanes[0].columns.map((c) => c.status), ["todo", "in_progress", "in_review", "done"]);
  const shown = boardLanes(issues, true);
  assert.deepEqual(shown.map(keys)[0], ["WEB-4", "WEB-6"]);
  assert.deepEqual(keys(shown.at(-1)), ["WEB-7", "WEB-2", "WEB-1", "WEB-8"]);
  assert.deepEqual(boardLanes([]), []);
});

test("the list by parent: a group per top-level parent, children indented in tree order, the rest loose", () => {
  const mk = (n, status, extra = {}) => ({ key: `WEB-${n}`, title: `t${n}`, type: "task", status, sessions: [], log: [], createdAt: 0, updatedAt: n, ...extra });
  const issues = [
    mk(1, "in_progress", { type: "epic" }),
    mk(2, "todo", { parent: "WEB-1", type: "feature" }),
    mk(3, "todo", { parent: "WEB-2" }),
    mk(4, "in_review", { parent: "WEB-1" }),
    mk(5, "todo"),
    mk(6, "canceled", { parent: "WEB-1" }),
    mk(7, "done", { type: "epic" }),
  ];
  const { groups, loose } = issueTree(issues);
  assert.deepEqual(groups.map((g) => g.root.key), ["WEB-1"]);
  assert.deepEqual(groups[0].rows.map((r) => `${r.issue.key}@${r.depth}`), ["WEB-4@1", "WEB-2@1", "WEB-3@2"]);
  // An epic with nothing under it is just a row, like any issue with no parent.
  assert.deepEqual(loose.map((i) => i.key), ["WEB-5", "WEB-7"]);
  assert.deepEqual(issueTree(issues, true).groups[0].rows.map((r) => r.issue.key), ["WEB-4", "WEB-2", "WEB-3", "WEB-6"]);
});

test("the breadcrumb trail adds the issue you leave, goes back to one already on it, and keeps the last eight", () => {
  assert.deepEqual(trailTo([], "WIN-12", "WIN-15"), ["WIN-12"]);
  assert.deepEqual(trailTo(["WIN-12"], "WIN-15", "WIN-20"), ["WIN-12", "WIN-15"]);
  assert.deepEqual(trailTo(["WIN-12", "WIN-15"], "WIN-20", "WIN-12"), []);
  assert.deepEqual(trailTo(["WIN-12", "WIN-15"], "WIN-20", "WIN-15"), ["WIN-12"]);
  const long = Array.from({ length: 8 }, (_, i) => `WIN-${i + 1}`);
  assert.deepEqual(trailTo(long, "WIN-9", "WIN-10"), [...long.slice(1), "WIN-9"]);
});
