import assert from "node:assert/strict";
import test from "node:test";
import { gtComments, gtItem, gtListed, gtOwn, gtQuoted, gtStatusOf } from "./gitea.ts";

const gt = (over = {}) => ({
  number: 9, title: "Fix y", state: "open", draft: false, merged: false, mergeable: true,
  comments: 2, review_comments: 1, additions: 10, deletions: 4, changed_files: 3,
  user: { login: "Me" }, requested_reviewers: [],
  head: { ref: "fix", sha: "deadbeef", repo: { full_name: "o/r" } }, base: { ref: "main" },
  updated_at: "2026-10-01T10:00:00+02:00", html_url: "https://codeberg.org/o/r/pulls/9",
  ...over,
});

test("Gitea status: merged, closed, draft, conflicts, checks and reviews decide the label", () => {
  assert.equal(gtStatusOf(gt({ state: "closed", merged: true }), [], []).label, "Merged");
  assert.equal(gtStatusOf(gt({ state: "closed" }), [], []).label, "Closed");
  assert.equal(gtStatusOf(gt({ draft: true }), [], []).label, "Draft");
  assert.equal(gtStatusOf(gt({ mergeable: false }), [], []).label, "Conflicts");
  const checks = gtStatusOf(gt(), [{ status: "failure", context: "ci / build", target_url: "/o/r/actions/runs/1", description: "Failing" }, { status: "success", context: "lint" }, { status: "skipped" }, { status: "pending", context: "e2e" }, { status: "warning" }], []);
  assert.deepEqual(checks.checks, { passed: 3, failed: 1, running: 1 });
  assert.equal(checks.label, "Checks failing");
  assert.deepEqual(checks.failing.map((f) => [f.id, f.name, f.summary]), [["ci / build", "ci / build", "Failing"]]);
  const reviews = [
    { id: 1, user: { login: "a" }, state: "REQUEST_CHANGES", submitted_at: "2026-10-01T09:00:00Z" },
    { id: 2, user: { login: "a" }, state: "APPROVED", submitted_at: "2026-10-01T10:00:00Z" },
    { id: 3, user: { login: "b" }, state: "REQUEST_CHANGES", submitted_at: "2026-10-01T10:00:00Z", dismissed: true },
    { id: 4, user: { login: "c" }, state: "REQUEST_REVIEW", submitted_at: "2026-10-01T10:00:00Z" },
  ];
  const reviewed = gtStatusOf(gt(), [], reviews);
  assert.equal(reviewed.label, "Approved");
  assert.equal(reviewed.review, "1 approved");
  assert.equal(gtStatusOf(gt(), [], [reviews[0]]).label, "Changes requested");
  const st = gtStatusOf(gt(), [], []);
  assert.deepEqual([st.key, st.branch, st.changes, st.label], ["deadbeef", "fix → main", "+10 −4 in 3 files", "In review"]);
});

test("Gitea statuses are checks", () => {
  assert.deepEqual(["success", "warning", "skipped", "pending", "error", "failure"].map((status) => gtItem({ status, context: "c" }).state), ["passed", "passed", "skipped", "running", "failed", "failed"]);
});

test("Gitea list keeps every open PR and marks the ones you wrote or were asked to review, matching the login without case", () => {
  const prs = [gt({ number: 1, user: { login: "other" } }), gt({ number: 2, user: { login: "other" }, requested_reviewers: [{ login: "ME" }] }), gt({ number: 3 }), gt({ number: 4, requested_reviewers: null, user: { login: "x" } })];
  const rows = gtListed(prs, "me", (pr) => ({ number: pr.number }));
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine, r.reviewing]), [[1, false, false], [2, false, true], [3, true, false], [4, false, false]]);
  assert.equal(rows[2].activity, 3);
  assert.equal(rows[2].head, "fix");
  assert.equal(gtListed([gt({ draft: true })], "me", (pr) => ({ number: pr.number }))[0].status.label, "Draft");
});

test("Gitea comments: the conversation and each review's summary, with negative ids for reviews", () => {
  const list = gtComments(
    [{ id: 10, user: { login: "a", full_name: "Ann" }, body: "hi", created_at: "t1" }],
    [{ id: 5, user: { login: "b" }, state: "APPROVED", body: "LGTM", submitted_at: "t2", html_url: "https://x/r5" }, { id: 6, user: { login: "c" }, state: "COMMENT", body: "", submitted_at: "t3" }, { id: 7, user: { login: "d" }, state: "PENDING", body: "draft", submitted_at: "t4" }],
    "https://codeberg.org/o/r/pulls/9",
  );
  assert.deepEqual(list.map((c) => [c.id, c.author, c.body, c.url]), [[10, "Ann", "hi", "https://codeberg.org/o/r/pulls/9"], [-5, "b", "LGTM", "https://x/r5"]]);
});

test("a branch lookup keeps only PRs from this repository, not a fork's branch of the same name", () => {
  const gts = [gt({ number: 1, head: { ref: "forgejo", sha: "a", repo: { full_name: "fork/r" } } }), gt({ number: 2, head: { ref: "forgejo", sha: "b", repo: { full_name: "O/R" } } }), gt({ number: 3, head: { ref: "forgejo", sha: "c", repo: null } })];
  assert.deepEqual(gtOwn(gts, "o/r").map((p) => p.number), [2]);
});

test("a Codeberg reply quotes the first lines of the comment it answers", () => {
  assert.equal(gtQuoted({ author: "Ana", body: "Why 2?" }, "Because."), "> **Ana** wrote:\n> Why 2?\n\nBecause.");
  const long = Array.from({ length: 9 }, (_, i) => `line ${i + 1}`).join("\n");
  const quoted = gtQuoted({ author: "Ana", body: `${long}\n` }, "ok");
  assert.equal(quoted.split("\n").filter((l) => l.startsWith("> line")).length, 6);
  assert.match(quoted, /> …\n\nok$/);
  assert.equal(gtQuoted({ author: "Ana", body: "a\n\nb" }, "ok"), "> **Ana** wrote:\n> a\n>\n> b\n\nok", "a blank line stays inside the quote");
});

test("Codeberg conversation comments take a reply", () => {
  const list = gtComments([{ id: 5, user: { login: "ana" }, body: "hi", created_at: "t", html_url: "https://codeberg.org/o/r/pulls/1#issuecomment-5" }], [], "https://codeberg.org/o/r/pulls/1");
  assert.equal(list[0].canReply, true);
});
