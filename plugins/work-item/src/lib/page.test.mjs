import assert from "node:assert/strict";
import test from "node:test";
import { allLinkIds, discussionOf, idsQuery, lastSegment, linkIds, linkedItems, prStatus, revisionsOf, summaryOf, tagsOf, LINKS_CAP } from "./item.ts";
import { counts, groupByDay, matches } from "./inbox.ts";
import { typeStyle } from "./type.ts";

const rel = (rel, id) => ({ rel, url: `https://dev.azure.com/org/_apis/wit/workItems/${id}`, attributes: { name: rel } });

test("linkIds groups the work item links by Azure's link types and ignores the rest", () => {
  const links = linkIds([
    rel("System.LinkTypes.Hierarchy-Reverse", 10),
    rel("System.LinkTypes.Hierarchy-Forward", 11),
    rel("System.LinkTypes.Hierarchy-Forward", 12),
    rel("System.LinkTypes.Hierarchy-Forward", 11),
    rel("System.LinkTypes.Related", 20),
    rel("System.LinkTypes.Dependency-Reverse", 30),
    rel("System.LinkTypes.Dependency-Forward", 31),
    rel("AttachedFile", 40),
    { rel: "Hyperlink", url: "https://example.com/x/1" },
    { rel: "System.LinkTypes.Related", url: "https://dev.azure.com/org/_apis/wit/workItems/abc" },
    { rel: "ArtifactLink", url: "vstfs:///Git/PullRequestId/p%2Fr%2F5", attributes: { name: "Pull Request" } },
  ]);
  assert.deepEqual(links, { parent: 10, children: [11, 12], related: [20], predecessors: [30], successors: [31] });
});

test("linkIds takes the parent from the field when the relations don't have it, and copes with no relations", () => {
  assert.equal(linkIds(null, 7).parent, 7);
  assert.equal(linkIds([rel("System.LinkTypes.Hierarchy-Reverse", 8)], 7).parent, 8);
  assert.deepEqual(linkIds(undefined), { parent: null, children: [], related: [], predecessors: [], successors: [] });
});

test("allLinkIds lists each id once, parent first, and stops at the cap", () => {
  assert.deepEqual(allLinkIds({ parent: 1, children: [2, 3], related: [3, 4], predecessors: [], successors: [5] }), [1, 2, 3, 4, 5]);
  const many = { parent: null, children: Array.from({ length: 100 }, (_, i) => i + 1), related: [], predecessors: [], successors: [] };
  assert.equal(allLinkIds(many).length, LINKS_CAP);
});

test("idsQuery only puts integers in the query", () => {
  assert.match(idsQuery([1, 22]), /\[System\.Id\] IN \(1,22\)$/);
  assert.match(idsQuery([1, NaN, -3, 1.5, 4]), /IN \(1,4\)$/);
});

test("linkedItems reads a query's rows by id, with the assignee as a name or a string", () => {
  const rows = [
    { id: 5, fields: { "System.Title": "Fix login", "System.State": "Active", "System.WorkItemType": "Bug", "System.AssignedTo": { displayName: "Ada Lovelace" } } },
    { id: 6, fields: { "System.AssignedTo": "Grace Hopper" } },
  ];
  assert.deepEqual(linkedItems(rows)[5], { id: 5, title: "Fix login", state: "Active", type: "Bug", assignee: "Ada Lovelace" });
  assert.deepEqual(linkedItems(rows)[6], { id: 6, title: "#6", state: "", type: "Work item", assignee: "Grace Hopper" });
});

test("prStatus words and tones", () => {
  assert.deepEqual(prStatus("active"), { label: "Active", tone: "info" });
  assert.deepEqual(prStatus("completed"), { label: "Completed", tone: "success" });
  assert.deepEqual(prStatus("abandoned"), { label: "Abandoned" });
  assert.deepEqual(prStatus(undefined), { label: "" });
});

test("summaryOf takes the estimate the process uses and drops what isn't set", () => {
  assert.deepEqual(summaryOf({ "Microsoft.VSTS.Common.Priority": 2, "Microsoft.VSTS.Scheduling.StoryPoints": 5, "Microsoft.VSTS.Scheduling.RemainingWork": 3 }), { priority: 2, estimate: { label: "Story points", value: 5 }, remaining: 3 });
  assert.deepEqual(summaryOf({ "Microsoft.VSTS.Scheduling.Effort": 8, "Microsoft.VSTS.Common.Severity": "2 - High" }), { severity: "High", estimate: { label: "Effort", value: 8 } });
  assert.deepEqual(summaryOf({}), {});
  assert.deepEqual(summaryOf({ "Microsoft.VSTS.Scheduling.RemainingWork": 0 }), { remaining: 0 });
});

test("lastSegment and tagsOf", () => {
  assert.equal(lastSegment("Demo\\Team A\\Sprint 5"), "Sprint 5");
  assert.equal(lastSegment(undefined), "");
  assert.deepEqual(tagsOf("api; ui ;  ;ops"), ["api", "ui", "ops"]);
  assert.deepEqual(tagsOf(undefined), []);
});

const upd = (id, at, by, fields) => ({ id, revisedBy: { displayName: by }, revisedDate: "9999-01-01T00:00:00Z", fields: { "System.ChangedDate": { newValue: at }, ...fields } });
const updates = [
  upd(3, "2026-10-03T10:00:00Z", "Grace Hopper", { "System.State": { oldValue: "Active", newValue: "Resolved" }, "System.History": { newValue: "<div>Fixed it</div>" } }),
  upd(1, "2026-10-01T10:00:00Z", "Ada Lovelace", { "System.State": { newValue: "New" }, "System.Title": { newValue: "Login" }, "System.Id": { newValue: 5 } }),
  upd(2, "2026-10-02T10:00:00Z", "Ada Lovelace", {
    "System.State": { oldValue: "New", newValue: "Active" },
    "System.AssignedTo": { oldValue: { displayName: "Ada Lovelace" }, newValue: { displayName: "Grace Hopper" } },
    "System.IterationPath": { oldValue: "Demo\\Sprint 4", newValue: "Demo\\Sprint 5" },
    "Microsoft.VSTS.Common.Priority": { oldValue: 2, newValue: 1 },
    "System.Rev": { oldValue: 1, newValue: 2 },
  }),
  upd(4, "2026-10-04T10:00:00Z", "Ada Lovelace", { "System.History": { newValue: "<div>Thanks</div>" } }),
  upd(5, "2026-10-05T10:00:00Z", "Ada Lovelace", { "System.AssignedTo": { oldValue: { displayName: "Grace Hopper" } } }),
];

test("revisionsOf lists what changed, oldest first, as text, and skips revisions that changed nothing we show", () => {
  const revs = revisionsOf(updates);
  assert.deepEqual(revs.map((r) => r.id), [1, 2, 3, 5]);
  assert.equal(revs[0].created, true);
  assert.deepEqual(revs[0].changes, [{ label: "State", to: "New" }, { label: "Title", to: "Login" }]);
  assert.deepEqual(revs[1].changes, [
    { label: "State", from: "New", to: "Active" },
    { label: "Assigned to", from: "Ada Lovelace", to: "Grace Hopper" },
    { label: "Sprint", from: "Sprint 4", to: "Sprint 5" },
    { label: "Priority", from: "2", to: "1" },
  ]);
  assert.deepEqual(revs[2].changes, [{ label: "State", from: "Active", to: "Resolved" }]);
  assert.deepEqual(revs[3].changes, [{ label: "Assigned to", from: "Grace Hopper" }]);
  assert.equal(revs[1].by, "Ada Lovelace");
});

test("revisionsOf of nothing is nothing", () => {
  assert.deepEqual(revisionsOf([]), []);
});

test("discussionOf merges comments with state, assignee and sprint changes in time order; other field changes stay in History", () => {
  const comments = [{ id: 3, author: "Grace Hopper", at: "2026-10-03T10:00:00Z", html: "<div>Fixed it</div>" }, { id: 4, author: "Ada Lovelace", at: "2026-10-04T10:00:00Z", html: "<div>Thanks</div>" }];
  const feed = discussionOf(comments, revisionsOf(updates));
  assert.deepEqual(feed.map((a) => a.key), ["r1", "r2", "c3", "r3", "c4", "r5"]);
  const created = feed[0];
  assert.equal(created.kind === "change" && created.created, true);
  const second = feed[1];
  assert.deepEqual(second.kind === "change" && second.changes.map((c) => c.label), ["State", "Assigned to", "Sprint"]);
});

test("typeStyle gives Azure's colours to the common types and a grey dot to the rest", () => {
  assert.deepEqual(typeStyle("Bug"), { glyph: "bug", color: "#cc293d" });
  assert.equal(typeStyle("User Story").glyph, "story");
  assert.equal(typeStyle("Product Backlog Item").glyph, "story");
  assert.equal(typeStyle("Task").glyph, "task");
  assert.equal(typeStyle("Test Case").glyph, "test");
  assert.equal(typeStyle("Epic").glyph, "epic");
  assert.equal(typeStyle("Whatever").glyph, "other");
});

const ev = (key, kind, at, read = false) => ({ key, kind, title: key, body: "", url: "", at: new Date(at).toISOString(), read });

test("the inbox filters keep what they say", () => {
  const events = [ev("a", "state", 1, true), ev("b", "mention", 2), ev("c", "pr-mention", 3), ev("d", "pr-vote", 4), ev("e", "pr-comment", 5, true)];
  const keys = (f) => events.filter((e) => matches(e, f)).map((e) => e.key);
  assert.deepEqual(keys("all"), ["a", "b", "c", "d", "e"]);
  assert.deepEqual(keys("unread"), ["b", "c", "d"]);
  assert.deepEqual(keys("mentions"), ["b", "c"]);
  assert.deepEqual(keys("state"), ["a"]);
  assert.deepEqual(keys("prs"), ["c", "d", "e"]);
  assert.deepEqual(counts(events), { all: 5, unread: 3, mentions: 2, state: 1, prs: 3 });
});

test("groupByDay: today, yesterday, then dates, newest first", () => {
  const now = new Date(2026, 9, 13, 15, 0).getTime();
  const at = (d, h) => new Date(2026, 9, d, h).getTime();
  const days = groupByDay([ev("old", "state", at(9, 8)), ev("t1", "state", at(13, 9)), ev("y1", "state", at(12, 23)), ev("t2", "state", at(13, 14))], now);
  assert.deepEqual(days.map((d) => d.label).slice(0, 2), ["Today", "Yesterday"]);
  assert.equal(days.length, 3);
  assert.deepEqual(days[0].events.map((e) => e.key), ["t2", "t1"]);
  assert.deepEqual(days[2].events.map((e) => e.key), ["old"]);
});

// ---------- a repo's Work items section ----------
import { branchHasId, isFinished, repoIdsOf, repoLinksOf, repoRows, stateCategories, NOT_LINKED } from "./sections.ts";
import { parseRemote } from "./branch.ts";

const TYPES = [
  { name: "Task", states: [{ name: "To Do", category: "Proposed" }, { name: "Done", category: "Completed" }, { name: "Removed", category: "Removed" }] },
  { name: "Bug", states: [{ name: "Active", category: "InProgress" }, { name: "Resolved", category: "Resolved" }, { name: "Closed", category: "Completed" }] },
];

test("isFinished follows the state's category: Completed and Removed are finished, Resolved is not", () => {
  const cats = stateCategories(TYPES);
  assert.equal(isFinished("Task", "Done", cats), true);
  assert.equal(isFinished("task", "removed", cats), true);
  assert.equal(isFinished("Task", "To Do", cats), false);
  assert.equal(isFinished("Bug", "Resolved", cats), false);
  assert.equal(isFinished("Bug", "Closed", cats), true);
});

test("isFinished falls back to the state's name without the categories or for an unknown type", () => {
  assert.equal(isFinished("Task", "Done"), true);
  assert.equal(isFinished("Task", "Resolved", undefined), false);
  assert.equal(isFinished("Task", "Active"), false);
  assert.equal(isFinished("Custom", "Closed", stateCategories(TYPES)), true);
  assert.equal(isFinished("Custom", "Doing", stateCategories(TYPES)), false);
  assert.deepEqual(stateCategories(null), {});
});

const art = (kind, repo, tail) => ({ rel: "ArtifactLink", url: `vstfs:///Git/${kind}/proj-guid%2F${repo}%2F${tail}` });

test("repoIdsOf reads the repository id out of branch, commit and pull request links, and nothing else", () => {
  assert.deepEqual(repoIdsOf([art("Ref", "AAA", "GBfeature%2F12-x"), art("Commit", "bbb", "abc123"), art("PullRequestId", "AAA", "7"), { rel: "ArtifactLink", url: "vstfs:///Build/Build/5" }, { rel: "Hyperlink", url: "https://x" }]), ["aaa", "bbb"]);
  assert.deepEqual(repoIdsOf(null), []);
  assert.deepEqual(repoLinksOf({ value: [{ id: 1, relations: [art("Ref", "R1", "GBx")] }, { id: 2, relations: null }] }), { 1: ["r1"], 2: [] });
});

test("branchHasId matches the id as a whole number", () => {
  assert.equal(branchHasId("feature/1234-login", 1234), true);
  assert.equal(branchHasId("origin/bugfix_1234", 1234), true);
  assert.equal(branchHasId("feature/12345-login", 1234), false);
  assert.equal(branchHasId("feature/123-login", 1234), false);
  assert.equal(branchHasId("feature/a1234", 1234), false);
  assert.equal(branchHasId("feature/12-x", 12), false);
  assert.equal(branchHasId("hotfix/2026-10-13", 2026), false);
  assert.equal(branchHasId("hotfix/2026-10-13", 10), false);
  assert.equal(branchHasId("release/2026.10", 2026), false);
  assert.equal(branchHasId("release/v1.200.3", 200), false);
  assert.equal(branchHasId("hotfix/2026-10-13-4521-fix", 4521), true);
});

const row = (id, state = "Active", type = "Task") => ({ id, fields: { "System.Title": `Item ${id}`, "System.State": state, "System.WorkItemType": type } });

test("repoRows: items linked to the repo or with a branch of it first, unlinked ones after under every repo, other repos' items left out", () => {
  const rows = [row(101), row(102), row(103), row(104), row(105)];
  const links = { 101: ["repo-a"], 102: ["repo-b"], 103: [], 104: ["repo-b"], 105: [] };
  const a = repoRows("https://dev.azure.com/org", "P", rows, links, { id: "REPO-A", branches: ["main", "feature/104-thing"] });
  assert.deepEqual(a.map((r) => r.id), ["101", "104", "103", "105"]);
  assert.doesNotMatch(a[0].detail, /Not linked/);
  assert.match(a[2].detail, new RegExp(NOT_LINKED));
  assert.match(a[3].detail, /Task #105 · Not linked to a repo/);
  const b = repoRows("https://dev.azure.com/org", "P", rows, links, { id: "repo-b", branches: [] });
  assert.deepEqual(b.map((r) => r.id), ["102", "104", "103", "105"]);
});

test("repoRows without the repo's id matches by branch only, and keeps at most 50 rows", () => {
  const rows = Array.from({ length: 80 }, (_, i) => row(i + 101));
  const out = repoRows("https://dev.azure.com/org", "P", rows, {}, { branches: ["feature/170-x"] });
  assert.equal(out.length, 50);
  assert.equal(out[0].id, "170");
  assert.equal(repoRows("https://dev.azure.com/org", "P", [row(101)], { 101: ["other"] }, { branches: [] }).length, 0);
});

test("parseRemote keeps the repository name", () => {
  assert.deepEqual(parseRemote("git@ssh.dev.azure.com:v3/org/My%20Proj/my-repo"), { kind: "azure", org: "https://dev.azure.com/org", project: "My Proj", repo: "my-repo" });
  assert.equal(parseRemote("https://org.visualstudio.com/DefaultCollection/P/_git/r.git").repo, "r");
});
