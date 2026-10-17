import assert from "node:assert/strict";
import test from "node:test";
import { hasLog } from "../checklist.ts";
import { ghItem, ghRun, ghState, ghWho, githubComment, githubConversation, githubListed, githubReviewers, githubStatusOf } from "./github.ts";

const pr = (over = {}) => ({
  number: 12, url: "https://github.com/o/r/pull/12", title: "Fix x", state: "OPEN", isDraft: false, reviewDecision: "", mergeable: "MERGEABLE",
  statusCheckRollup: [], additions: 5, deletions: 2, changedFiles: 3, headRefName: "fix", baseRefName: "main", updatedAt: "2026-10-01T10:00:00Z", author: { login: "ada" },
  ...over,
});
const run = (conclusion, name = "unit") => ({ __typename: "CheckRun", name, status: "COMPLETED", conclusion, detailsUrl: `https://github.com/o/r/actions/runs/9/job/${name.length}` });

test("GitHub status: the first thing that is true names the PR, from merged down to the review decision", () => {
  const label = (over) => githubStatusOf(pr(over)).label;
  assert.equal(label({ state: "MERGED", isDraft: true }), "Merged");
  assert.equal(label({ state: "CLOSED", isDraft: true }), "Closed");
  assert.equal(label({ isDraft: true, mergeable: "CONFLICTING" }), "Draft");
  assert.equal(label({ mergeable: "CONFLICTING", statusCheckRollup: [run("FAILURE")] }), "Conflicts");
  assert.equal(label({ statusCheckRollup: [run("FAILURE")], reviewDecision: "CHANGES_REQUESTED" }), "Checks failing");
  assert.equal(label({ reviewDecision: "CHANGES_REQUESTED", statusCheckRollup: [{ ...run("SUCCESS"), status: "IN_PROGRESS" }] }), "Changes requested");
  assert.equal(label({ statusCheckRollup: [{ __typename: "CheckRun", name: "e2e", status: "IN_PROGRESS" }], reviewDecision: "APPROVED" }), "Checks running");
  assert.equal(label({ reviewDecision: "APPROVED" }), "Approved");
  assert.equal(label({ reviewDecision: "REVIEW_REQUIRED" }), "In review");
  assert.equal(label({}), "Open", "a PR with no review policy has nothing to wait for");
  assert.deepEqual([githubStatusOf(pr({ state: "MERGED" })).tone, githubStatusOf(pr({ state: "MERGED" })).icon], ["merged", "pr-merged"]);
});

test("GitHub status carries the key, branches, size, review text and the failed jobs that have a log", () => {
  const st = githubStatusOf(pr({ reviewDecision: "REVIEW_REQUIRED", statusCheckRollup: [run("SUCCESS", "lint"), run("FAILURE", "unit"), { __typename: "StatusContext", context: "legacy", state: "SUCCESS" }] }));
  assert.deepEqual([st.key, st.title, st.branch, st.changes, st.review, st.updated], ["2026-10-01T10:00:00Z", "Fix x", "fix → main", "+5 −2 in 3 files", "Waiting for review", "2026-10-01T10:00:00Z"]);
  assert.deepEqual(st.checks, { passed: 2, failed: 1, running: 0 });
  assert.deepEqual(st.failing.map((f) => [f.name, f.job]), [["unit", "4"]]);
  assert.equal(st.items.length, 3);
  assert.equal(githubStatusOf(pr({ reviewDecision: "CHANGES_REQUESTED" })).review, "Changes requested");
  assert.equal(githubStatusOf(pr()).review, "No review yet");
});

test("GitHub list marks the PRs you wrote or are asked to review, matching the login without case, and carries the full status", () => {
  const prs = [
    pr({ number: 1, author: { login: "Me" } }),
    pr({ number: 2, reviewRequests: [{ login: "ME" }] }),
    pr({ number: 3, reviewRequests: [{}] }),
    pr({ number: 4, reviewRequests: [{ slug: "team" }], comments: [1, 2], reviews: [1] }),
  ];
  const rows = githubListed(prs, "me", new Set([4]));
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine, r.reviewing]), [[1, true, false], [2, false, true], [3, false, false], [4, false, true]]);
  assert.deepEqual([rows[0].link, rows[0].head, rows[0].author, rows[0].loaded, rows[3].activity], [{ provider: "github", url: "https://github.com/o/r/pull/12", number: 1, repo: "o/r" }, "fix", "Me", true, 3]);
  assert.equal(rows[0].status.title, "Fix x");
});

test("GitHub: you wrote it, or your login is among the review requests (teams have no login)", () => {
  assert.deepEqual(ghWho({ author: { login: "Me" }, reviewRequests: [] }, "me"), { mine: true, reviewing: false });
  assert.deepEqual(ghWho({ author: { login: "ann" }, reviewRequests: [{ login: "bob" }, { login: "ME" }] }, "me"), { mine: false, reviewing: true });
  assert.deepEqual(ghWho({ author: { login: "ann" }, reviewRequests: [{}] }, "me"), { mine: false, reviewing: false });
});

test("GitHub: a review request to your team counts through the search's numbers", () => {
  assert.equal(ghWho({ number: 7, author: { login: "ann" }, reviewRequests: [{}] }, "me", new Set([7])).reviewing, true);
  assert.equal(ghWho({ number: 8, author: { login: "ann" }, reviewRequests: [{}] }, "me", new Set([7])).reviewing, false);
});

test("a review's verdict is a reviewer's vote, and a comment-only review is none", () => {
  assert.deepEqual(githubReviewers([{ author: { login: "a" }, state: "APPROVED" }, { author: {}, state: "CHANGES_REQUESTED" }, { author: { login: "c" }, state: "COMMENTED" }]), [{ name: "a", vote: "approved" }, { name: "unknown", vote: "changes" }]);
  assert.deepEqual(githubReviewers(undefined), []);
});

test("GitHub conversation: comments on the whole PR first, with ids that can't clash with line comments, then the line comments", () => {
  const discussion = {
    comments: [{ author: { login: "ada" }, body: "hi", createdAt: "t1", url: "https://github.com/o/r/pull/12#c1" }, { body: "from nobody", createdAt: "t2", url: "u2" }],
    reviews: [{ author: { login: "bob" }, body: "LGTM", submittedAt: "t3" }, { author: { login: "cy" }, body: "", submittedAt: "t4" }],
  };
  const line = { id: 55, path: "a.ts", line: 4, side: "RIGHT", user: { login: "dee" }, body: "nit", created_at: "t5", html_url: "u5" };
  const list = githubConversation(discussion, [line], "https://github.com/o/r/pull/12");
  assert.deepEqual(list.map((c) => [c.id, c.author, c.path, c.url]), [
    [-1, "ada", null, "https://github.com/o/r/pull/12#c1"],
    [-2, "unknown", null, "u2"],
    [-3, "bob", null, "https://github.com/o/r/pull/12"],
    [55, "dee", "a.ts", "u5"],
  ]);
  assert.equal(list[0].canReply, undefined, "the whole-PR ones take no reply");
});

test("a GitHub line comment sits on a side, and only a thread's first comment takes a reply", () => {
  const first = githubComment({ id: 1, path: "a.ts", line: 3, side: "LEFT", user: { login: "a" }, body: "x", created_at: "t", html_url: "u" });
  assert.deepEqual([first.side, first.replyTo, first.canReply, first.line], ["deletions", null, true, 3]);
  const reply = githubComment({ id: 2, in_reply_to_id: 1, path: "a.ts", side: "RIGHT", body: undefined, created_at: "t", html_url: "u" });
  assert.deepEqual([reply.side, reply.replyTo, reply.canReply, reply.line, reply.author, reply.body], ["additions", 1, false, null, "unknown", ""]);
});

test("GitHub check runs and status contexts map to a state", () => {
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "SUCCESS" }), "passed");
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "NEUTRAL" }), "passed");
  assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion: "SKIPPED" }), "skipped");
  for (const conclusion of ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"]) assert.equal(ghState({ __typename: "CheckRun", status: "COMPLETED", conclusion }), "failed", conclusion);
  assert.equal(ghState({ __typename: "CheckRun", status: "IN_PROGRESS" }), "running");
  for (const status of ["QUEUED", "WAITING", "PENDING", "REQUESTED"]) assert.equal(ghState({ __typename: "CheckRun", status }), "queued", status);
  assert.equal(ghState({ __typename: "StatusContext", state: "SUCCESS" }), "passed");
  assert.equal(ghState({ __typename: "StatusContext", state: "ERROR" }), "failed");
  assert.equal(ghState({ __typename: "StatusContext", state: "PENDING" }), "running");
  assert.equal(ghState({ __typename: "StatusContext", state: "EXPECTED" }), "queued");
});

test("a failed Actions job on the PR's host can be read, another app's check only links out", () => {
  const job = ghItem({ __typename: "CheckRun", name: "unit", workflowName: "CI", status: "COMPLETED", conclusion: "FAILURE", detailsUrl: "https://github.com/o/r/actions/runs/9/job/77", startedAt: "2026-01-01T00:00:00Z", completedAt: "2026-01-01T00:01:30Z" }, "github.com");
  assert.equal(job.failing.job, "77");
  assert.equal(hasLog(job.failing), true);
  assert.equal(job.kind, "CI");
  assert.equal(job.seconds, 90);
  assert.equal(job.at, "2026-01-01T00:01:30Z");
  const app = ghItem({ __typename: "CheckRun", name: "deploy", status: "COMPLETED", conclusion: "TIMED_OUT", detailsUrl: "https://ci.example.com/b/1" }, "github.com");
  assert.equal(hasLog(app.failing), false);
  assert.equal(app.note, "Timed out");
  const ctx = ghItem({ __typename: "StatusContext", context: "legacy/ci", state: "SUCCESS", targetUrl: "https://ci.example.com/2", startedAt: "0001-01-01T00:00:00Z" }, "github.com");
  assert.equal(ctx.kind, "Status check");
  assert.equal(ctx.at, null);
  assert.equal(ctx.failing, undefined);
});

test("two GitHub contexts that share a link and even a name get different ids", () => {
  const ctx = (context, i) => ghItem({ __typename: "StatusContext", context, state: "SUCCESS", targetUrl: "https://codecov.example/pr/1" }, "github.com", i);
  assert.notEqual(ctx("codecov/patch", 0).id, ctx("codecov/project", 1).id);
  assert.notEqual(ctx("codecov/patch", 0).id, ctx("codecov/patch", 1).id);
});

test("ghRun gives no step a log until the job finishes, because GitHub has none before", () => {
  const step = { number: 1, name: "Set up job", status: "completed", conclusion: "success", started_at: "2026-10-17T07:00:00Z", completed_at: "2026-10-17T07:00:02Z" };
  const running = ghRun({ id: 7, name: "test", status: "in_progress", conclusion: null, steps: [step] });
  assert.equal(running.jobs[0].steps[0].log, null);
  assert.match(running.note, /once the job finishes/);
  const done = ghRun({ id: 7, name: "test", status: "completed", conclusion: "success", steps: [step] });
  assert.equal(done.jobs[0].steps[0].log, "1");
  assert.equal(done.note, undefined);
});
