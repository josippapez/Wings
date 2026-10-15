import assert from "node:assert/strict";
import test from "node:test";
import { azWho, azureComments, azureItems, azureLink, azureListed, azureReplied, azureReviewers, azureStatus, azureStatusOf } from "./azure.ts";

const org = "https://dev.azure.com/acme";
const remote = { kind: "azure", org, project: "My Project", repo: "app" };
const pr = (over = {}) => ({
  pullRequestId: 42, status: "active", title: "Add z", reviewers: [], creationDate: "2026-10-01T09:00:00Z",
  lastMergeSourceCommit: { commitId: "a".repeat(40) }, lastMergeTargetCommit: { commitId: "b".repeat(40) },
  sourceRefName: "refs/heads/feat", targetRefName: "refs/heads/main", createdBy: { displayName: "Ada", uniqueName: "ada@example.com" },
  ...over,
});
const none = { passed: 0, failed: 0, running: 0 };
const policy = (type, status, extra = {}) => ({ status, configuration: { isEnabled: true, isBlocking: true, type: { id: "t", displayName: type }, settings: {}, ...extra.configuration }, ...extra.rest });

test("Azure status words its own states: abandoned, rejected and waiting for the author", () => {
  const label = (over, checks = none) => azureStatus(pr(over), checks).label;
  assert.equal(label({ status: "completed", isDraft: true }), "Merged");
  assert.equal(label({ status: "abandoned" }), "Abandoned");
  assert.equal(label({ isDraft: true, mergeStatus: "conflicts" }), "Draft");
  assert.equal(label({ mergeStatus: "conflicts", reviewers: [{ vote: -10 }] }), "Conflicts");
  assert.equal(label({ reviewers: [{ vote: -10 }, { vote: 10 }] }, { ...none, failed: 1 }), "Rejected");
  assert.equal(label({ reviewers: [{ vote: -5 }] }, { ...none, failed: 1 }), "Checks failing");
  assert.equal(label({ reviewers: [{ vote: -5 }, { vote: 10 }] }, { ...none, running: 1 }), "Waiting for author");
  assert.equal(label({ reviewers: [{ vote: 5 }] }, { ...none, running: 1 }), "Checks running");
  assert.equal(label({ reviewers: [{ vote: 5 }] }), "Approved");
  assert.equal(label({ reviewers: [{ vote: 0 }] }), "In review");
  assert.deepEqual([azureStatus(pr({ status: "abandoned" }), none).tone, azureStatus(pr({ status: "abandoned" }), none).icon], ["neutral", "pr-closed"]);
});

test("Azure status: the review text counts votes, and the key, branches and commits come from the PR", () => {
  const st = azureStatus(pr({ reviewers: [{ vote: 10 }, { vote: 5 }, { vote: -5 }, { vote: -10 }, { vote: 0 }] }), none);
  assert.equal(st.review, "2 approved, 1 waiting for author, 1 rejected");
  assert.equal(azureStatus(pr(), none).review, "No votes yet");
  assert.deepEqual([st.key, st.branch, st.base, st.head, st.baseCommit, st.headCommit, st.updated], ["a".repeat(40), "feat → main", "main", "feat", "b".repeat(40), "a".repeat(40), "2026-10-01T09:00:00Z"]);
  assert.equal(azureStatus(pr({ lastMergeSourceCommit: undefined, status: "completed", closedDate: "2026-10-02T00:00:00Z" }), none).key, "2026-10-01T09:00:00Z");
  assert.equal(azureStatus(pr({ closedDate: "2026-10-02T00:00:00Z" }), none).updated, "2026-10-02T00:00:00Z");
});

test("Azure status from policies: not applicable and disabled ones don't count, a build names its failure, the merge strategy has its own row", () => {
  const records = [
    policy("Build", "rejected", { configuration: { settings: { displayName: "CI" } }, rest: { context: { buildId: 42 } } }),
    policy("Build", "approved", { rest: { context: { buildId: 43, buildDefinitionName: "Lint" } } }),
    policy("Status", "notApplicable"),
    policy("Build", "running", { configuration: { isEnabled: false } }),
    policy("Comment requirements", "broken"),
    policy("Minimum number of reviewers", "queued", { configuration: { type: { id: "fa4e907d-c16b-4a4c-9dfa-4906e5d171dd", displayName: "Minimum number of reviewers" } } }),
    policy("Require a merge strategy", "rejected", { configuration: { type: { id: "fa4e907d-c16b-4a4c-9dfa-4916e5d171ab", displayName: "Require a merge strategy" }, settings: { allowSquash: true, allowRebase: true } } }),
  ];
  const st = azureStatusOf(pr(), records, remote);
  assert.deepEqual(st.checks, { passed: 1, failed: 2, running: 0 });
  assert.deepEqual(st.failing, [{ id: 42, name: "CI", build: 42 }, { id: "Comment requirements", name: "Comment requirements", build: null }]);
  assert.equal(st.merge, "Squash or Rebase only, pick it when completing");
  assert.equal(st.label, "Checks failing");
  assert.equal(st.items.length, 6, "the checks view lists every enabled policy, the merge strategy too");
  assert.equal(azureStatusOf(pr(), [], remote).merge, null);
});

test("Azure policies: required reviewers say who is missing, a build links to its run", () => {
  const reviewers = { reviewers: [{ vote: 10, displayName: "Ada", isRequired: true }, { vote: 0, displayName: "Grace", isRequired: true }, { vote: 5, displayName: "Alan" }] };
  const items = azureItems(
    [
      policy("Required reviewers", "queued"),
      policy("Build", "rejected", { configuration: { settings: { displayName: "CI" } }, rest: { context: { buildId: 42 }, startedDate: "2026-01-01T00:00:00Z", completedDate: "2026-01-01T00:03:00Z" } }),
      policy("Work item linking", "rejected"),
      policy("Comment requirements", "rejected"),
      policy("Minimum number of reviewers", "queued", { configuration: { settings: { minimumApproverCount: 2 } } }),
      policy("Status", "notApplicable", { configuration: { isBlocking: false } }),
      policy("Build", "running", { configuration: { isEnabled: false } }),
    ],
    reviewers,
    org,
    "My Project",
  );
  const by = (kind) => items.find((c) => c.kind === kind);
  assert.equal(items.length, 6, "a disabled policy is not a check");
  assert.equal(by("Required reviewers").note, "1 of 2 required reviewers approved. Waiting for Grace");
  assert.equal(by("Minimum reviewers").note, "2 of 2 approvals");
  const build = by("Build");
  assert.deepEqual([build.name, build.state, build.required, build.seconds, build.url, build.failing], ["CI", "failed", true, 180, `${org}/My%20Project/_build/results?buildId=42`, { id: 42, name: "CI", build: 42 }]);
  assert.equal(by("Work item linking").state, "failed");
  assert.equal(by("Work item linking").failing, undefined);
  assert.equal(by("Comment requirements").needs, "unresolved");
  const na = by("Status check");
  assert.deepEqual([na.state, na.required], ["skipped", false]);
});

test("an unknown Azure policy keeps the name Azure gives it", () => {
  const [c] = azureItems([policy("File size restriction", "approved")], {}, org, "p");
  assert.deepEqual([c.name, c.kind, c.state], ["File size restriction", "File size restriction", "passed"]);
});

test("the merge strategy policy waits instead of failing", () => {
  const [c] = azureItems([policy("Require a merge strategy", "rejected")], {}, org, "p");
  assert.deepEqual([c.state, c.note], ["queued", "Pick the merge type when completing"]);
});

test("Azure reviewers vote approved, waiting, rejected or not at all", () => {
  assert.deepEqual(azureReviewers(pr({ reviewers: [{ vote: 10, displayName: "A" }, { vote: 5, displayName: "B" }, { vote: -5 }, { vote: -10, displayName: "D" }, { vote: 0, displayName: "E" }] })).map((r) => [r.name, r.vote]), [["A", "approved"], ["B", "approved"], ["unknown", "waiting"], ["D", "changes"], ["E", "none"]]);
});

test("Azure list marks the PRs you created or were asked to review, by unique name without case, and says the list has no check counts", () => {
  const prs = [pr({ pullRequestId: 1, createdBy: { displayName: "Me", uniqueName: "ME@example.com" } }), pr({ pullRequestId: 2, reviewers: [{ vote: 0, uniqueName: "me@EXAMPLE.com" }] }), pr({ pullRequestId: 3 })];
  const rows = azureListed(prs, "me@example.com", remote, "/work/app");
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine, r.reviewing]), [[1, true, false], [2, false, true], [3, false, false]]);
  assert.deepEqual([rows[0].head, rows[0].author, rows[0].loaded, rows[0].activity, rows[0].status.failing], ["feat", "Me", false, null, []]);
  assert.deepEqual(azWho({ createdBy: { uniqueName: "ann@example.com" }, reviewers: [{}] }, "me@example.com"), { mine: false, reviewing: false });
});

test("an Azure link has the PR's web address, else one built from the organization", () => {
  const link = azureLink(pr({ repository: { webUrl: "https://acme.visualstudio.com/My%20Project/_git/app" } }), remote, "/work/app");
  assert.deepEqual([link.url, link.number, link.repo, link.cwd, link.provider], ["https://acme.visualstudio.com/My%20Project/_git/app/pullrequest/42", 42, "My Project/app", "/work/app", "azure"]);
  assert.equal(azureLink(pr(), remote, "/w").url, `${org}/My%20Project/_git/app/pullrequest/42`);
});

test("Azure threads: ids carry the thread, replies sit under the first comment, system notes only when asked for", () => {
  const threads = [
    { id: 3, status: "active", threadContext: { filePath: "/src/a.ts", rightFileStart: { line: 8 }, rightFileEnd: { line: 10 } }, pullRequestThreadContext: { iterationContext: { secondComparingIteration: 2 } }, comments: [
      { id: 1, commentType: "text", author: { displayName: "Ann" }, content: "why?", publishedDate: "t1" },
      { id: 2, commentType: "text", author: { displayName: "Bob" }, content: "because", publishedDate: "t2" },
      { id: 3, commentType: "text", isDeleted: true, content: "oops", publishedDate: "t3" },
    ] },
    { id: 4, isDeleted: true, comments: [{ id: 1, commentType: "text", content: "gone", publishedDate: "t4" }] },
    { id: 5, status: "fixed", threadContext: { filePath: "/b.ts", leftFileStart: { line: 2 } }, comments: [{ id: 1, commentType: "text", author: {}, content: "old", publishedDate: "t5" }] },
    { id: 6, comments: [{ id: 1, commentType: "system", author: { displayName: "Azure" }, content: "Policy updated", publishedDate: "t6" }, { id: 2, commentType: "text", content: "whole PR", publishedDate: "t7" }] },
  ];
  const url = `${org}/My%20Project/_git/app/pullrequest/42`;
  const plain = azureComments(threads, url);
  assert.deepEqual(plain.map((c) => [c.id, c.replyTo, c.path, c.line, c.side, c.canReply, c.thread?.label]), [
    [30001, null, "src/a.ts", 8, "additions", true, "Active"],
    [30002, 30001, "src/a.ts", 8, "additions", false, undefined],
    [50001, null, "b.ts", 2, "deletions", true, "Resolved"],
    [60002, null, null, null, "deletions", true, undefined],
  ]);
  assert.deepEqual(plain[0].range, { start: 8, end: 10, iteration: 2 });
  assert.deepEqual([plain[2].author, plain[2].range], ["unknown", { start: 2, end: 2, iteration: undefined }]);
  const all = azureComments(threads, url, true);
  const note = all.find((c) => c.id === 60001);
  assert.deepEqual([all.length, note.system, note.canReply, note.replyTo, all.find((c) => c.id === 60002).replyTo], [5, true, false, null, 60001]);
});

test("an Azure reply keeps the thread's file and is a reply to its first comment, with the thread in its id", () => {
  const root = { id: 30001, replyTo: null, path: "src/a.ts", line: 8, side: "additions", author: "Ann", body: "why?", createdAt: "t1", url: "u", canReply: true, thread: { state: "active", label: "Active" } };
  const reply = azureReplied(root, { id: 7, author: { displayName: "Me" }, content: "because", publishedDate: "t9" }, "because");
  assert.deepEqual([reply.id, reply.replyTo, reply.path, reply.author, reply.canReply, reply.thread.label], [30007, 30001, "src/a.ts", "Me", false, "Active"]);
  assert.equal(azureReplied(root, { id: 8, publishedDate: "t" }, "typed").body, "typed");
  assert.equal(azureReplied(root, { id: 8, publishedDate: "t" }, "typed").author, "you");
});
