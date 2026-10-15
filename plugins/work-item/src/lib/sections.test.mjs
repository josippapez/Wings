import assert from "node:assert/strict";
import test from "node:test";
import { assignedQuery, blockItems, cleanBlockState, currentIterations, groupByState, iterationPath, sectionRows, sprintQuery, stateTone } from "./sections.ts";
import { commentsOf, itemArgs, itemTabArgs, pullRequestLinks } from "./item.ts";

const NOW = Date.parse("2026-10-13T10:00:00Z");
const node = (name, start, finish, children = [], parent = "\\Proj\\Iteration") => ({
  name,
  path: `${parent}\\${name}`,
  attributes: start ? { startDate: start, finishDate: finish } : undefined,
  children,
});

test("iterationPath drops the Iteration segment", () => {
  assert.equal(iterationPath("\\Proj\\Iteration\\Sprint 5"), "Proj\\Sprint 5");
  assert.equal(iterationPath("\\Proj\\Iteration"), "Proj");
});

test("currentIterations: running by date, last day included, undated skipped", () => {
  const root = { path: "\\Proj\\Iteration", children: [
    node("Sprint 4", "2026-09-15T00:00:00Z", "2026-09-28T00:00:00Z"),
    node("Sprint 5", "2026-10-12T00:00:00Z", "2026-10-13T00:00:00Z"),
    node("Backlog"),
  ] };
  assert.deepEqual(currentIterations(root, NOW), ["Proj\\Sprint 5"]);
  assert.deepEqual(currentIterations(root, Date.parse("2026-10-14T00:00:01Z")), []);
  assert.deepEqual(currentIterations(null, NOW), []);
});

test("currentIterations: a running parent with a running child gives way to the child", () => {
  const sprint = node("Sprint 5", "2026-10-12T00:00:00Z", "2026-10-26T00:00:00Z", [], "\\Proj\\Iteration\\Release 2");
  const root = { path: "\\Proj\\Iteration", children: [node("Release 2", "2026-09-01T00:00:00Z", "2026-12-01T00:00:00Z", [sprint])] };
  assert.deepEqual(currentIterations(root, NOW), ["Proj\\Release 2\\Sprint 5"]);
});

test("assignedQuery quotes names", () => {
  const q = assignedQuery("O'Neil", ["O'Neil\\Sprint 1"]);
  assert.match(q, /\[System\.TeamProject\] = 'O''Neil'/);
  assert.match(q, /IN \('O''Neil\\Sprint 1'\)/);
  assert.match(q, /@Me/);
});

test("sectionRows: at most 50, state badge, opens the item view", () => {
  const rows = Array.from({ length: 60 }, (_, i) => ({ id: i + 1, fields: { "System.Title": `T${i}`, "System.State": "Active", "System.WorkItemType": "Task" } }));
  const out = sectionRows("https://dev.azure.com/o", rows);
  assert.equal(out.length, 50);
  assert.deepEqual(out[0], { id: "1", label: "T0", detail: "Task #1", badge: { text: "Active", tone: "info" }, open: { view: "item", args: { id: 1, org: "https://dev.azure.com/o" } } });
});

test("a work item's tab opens with the same args from the badge, a section row, the Sprint block and the item page", () => {
  const org = "https://dev.azure.com/o";
  // The badge has the id from the branch name, as text; the section rows, the Sprint block and the item page have a number.
  const fromBadge = itemTabArgs("1234", org);
  const fromSection = sectionRows(org, [{ id: 1234, fields: {} }])[0].open.args;
  const fromBlock = itemTabArgs(1234, org);
  assert.deepEqual(fromBadge, { id: 1234, org });
  assert.equal(JSON.stringify(fromSection), JSON.stringify(fromBadge));
  assert.equal(JSON.stringify(fromBlock), JSON.stringify(fromBadge));
  assert.deepEqual(itemArgs(fromBadge), fromBadge);
});

test("stateTone", () => {
  assert.deepEqual(["Done", "Resolved", "Ready for Peer Review/QA", "Active", "New"].map(stateTone), ["success", "merged", "warning", "info", "neutral"]);
});

test("pullRequestLinks reads the vstfs artifact links, once each", () => {
  const link = (id) => ({ rel: "ArtifactLink", attributes: { name: "Pull Request" }, url: `vstfs:///Git/PullRequestId/aaa%2fbbb%2f${id}` });
  const rels = [link(12), link(12), { rel: "ArtifactLink", attributes: { name: "Fixed in Commit" }, url: "vstfs:///Git/Commit/aaa%2fbbb%2fabc" }, { rel: "Related" }];
  assert.deepEqual(pullRequestLinks(rels), [{ repoId: "bbb", id: 12 }]);
  assert.deepEqual(pullRequestLinks(null), []);
});

test("commentsOf keeps revisions with a History, oldest first", () => {
  const u = (id, at, history) => ({ id, revisedBy: { displayName: "Ana" }, fields: { "System.ChangedDate": { newValue: at }, ...(history ? { "System.History": { newValue: history } } : {}) } });
  const out = commentsOf([u(3, "2026-10-03T00:00:00Z", "<p>b</p>"), u(2, "2026-10-02T00:00:00Z"), u(1, "2026-10-01T00:00:00Z", "<p>a</p>")]);
  assert.deepEqual(out.map((c) => [c.id, c.html]), [[1, "<p>a</p>"], [3, "<p>b</p>"]]);
});

test("itemArgs accepts only an id and an https org", () => {
  assert.deepEqual(itemArgs({ id: 5, org: "https://dev.azure.com/o" }), { id: 5, org: "https://dev.azure.com/o" });
  assert.deepEqual(itemArgs({ id: 5, org: "https://dev.azure.com/o", project: "P" }), { id: 5, org: "https://dev.azure.com/o" }, "a tab saved with a project still opens");
  for (const bad of [null, {}, { id: "5", org: "https://x" }, { id: 5, org: "--x" }, { id: 0, org: "https://x" }]) assert.equal(itemArgs(bad), null);
});

test("sprintQuery has no assignee filter and asks for the assignee", () => {
  const q = sprintQuery("O'Neil", ["O'Neil\\Sprint 1", "O'Neil\\Sprint 2"]);
  assert.doesNotMatch(q, /@Me/);
  assert.match(q, /\[System\.AssignedTo\] FROM/);
  assert.match(q, /IN \('O''Neil\\Sprint 1', 'O''Neil\\Sprint 2'\)/);
});

test("blockItems: capped at 100, assignee from an identity or a string", () => {
  const row = (id, who) => ({ id, fields: { "System.Title": `T${id}`, "System.State": "Active", "System.WorkItemType": "Bug", ...(who ? { "System.AssignedTo": who } : {}) } });
  const out = blockItems([row(1, { displayName: "Ana" }), row(2, "Bo"), row(3), ...Array.from({ length: 120 }, (_, i) => row(i + 10))]);
  assert.equal(out.length, 100);
  assert.deepEqual(out.slice(0, 3).map((i) => i.assignee), ["Ana", "Bo", undefined]);
  assert.deepEqual(out[2], { id: 3, title: "T3", type: "Bug", state: "Active" });
});

test("groupByState: columns in workflow order, items keep their order", () => {
  const it = (id, state) => ({ id, title: `T${id}`, type: "Task", state });
  const cols = groupByState([it(1, "Done"), it(2, "Active"), it(3, "New"), it(4, "Active"), it(5, "Ready for QA")]);
  assert.deepEqual(cols.map((c) => c.state), ["New", "Active", "Ready for QA", "Done"]);
  assert.deepEqual(cols[1].items.map((i) => i.id), [2, 4]);
  assert.deepEqual(groupByState([]), []);
});

test("cleanBlockState is yours unless it says otherwise", () => {
  assert.deepEqual(cleanBlockState(null), { mine: true });
  assert.deepEqual(cleanBlockState({ mine: false }), { mine: false });
  assert.deepEqual(cleanBlockState({ mine: "no" }), { mine: true });
});
