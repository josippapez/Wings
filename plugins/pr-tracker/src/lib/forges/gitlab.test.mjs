import assert from "node:assert/strict";
import test from "node:test";
import { glComments, glItem, glListStatus, glListed, glReplied, glReviewers, glStatusOf, glWho } from "./gitlab.ts";

const mr = (over = {}) => ({
  iid: 5, web_url: "https://gitlab.example.com/g/p/-/merge_requests/5", state: "opened", title: "Add y", author: { name: "Ada", username: "ada" }, reviewers: [],
  user_notes_count: 4, sha: "c0ffee", source_branch: "feat", target_branch: "main", updated_at: "2026-10-01T10:00:00Z", changes_count: "7", head_pipeline: { id: 31, project_id: 3 },
  ...over,
});
const job = (status, extra = {}) => ({ id: 5, name: "test", status, stage: "verify", web_url: "https://gitlab.example.com/j/5", duration: 61.2, ...extra });

test("GitLab status: the first thing that is true names the MR, from merged down to approved", () => {
  const label = (over, jobs = [], approvals = null) => glStatusOf(mr(over), approvals, jobs).label;
  assert.equal(label({ state: "merged", draft: true }), "Merged");
  assert.equal(label({ state: "locked" }), "Closed");
  assert.equal(label({ draft: true, has_conflicts: true }), "Draft");
  assert.equal(label({ detailed_merge_status: "conflict" }, [job("failed")]), "Conflicts");
  assert.equal(label({ detailed_merge_status: "requested_changes" }, [job("failed")]), "Checks failing");
  assert.equal(label({ detailed_merge_status: "requested_changes" }, [job("running")]), "Changes requested");
  assert.equal(label({ detailed_merge_status: "discussions_not_resolved" }, [job("running")]), "Checks running");
  assert.equal(label({ detailed_merge_status: "discussions_not_resolved" }, [], { approved: true }), "Unresolved threads");
  assert.equal(label({}, [], { approved: true }), "Approved");
  assert.equal(label({}), "In review");
});

test("GitLab status: manual jobs don't count, allowed failures pass, canceled ones fail, and the review says who is missing", () => {
  const jobs = [job("success", { id: 1 }), job("manual", { id: 2 }), job("failed", { id: 3, allow_failure: true }), job("canceled", { id: 4 }), job("running", { id: 6 }), job("failed", { id: 7 })];
  const st = glStatusOf(mr(), { approved_by: [{ user: { username: "bob" } }], approvals_left: 2 }, jobs);
  assert.deepEqual(st.checks, { passed: 2, failed: 2, running: 1 });
  assert.deepEqual(st.failing.map((f) => [f.id, f.summary, f.project]), [[4, "Canceled", 3], [7, null, 3]]);
  assert.equal(st.items.length, 6, "manual jobs are listed, though they don't count");
  assert.deepEqual([st.review, st.key, st.branch, st.changes], ["1 approved, 2 more needed", "c0ffee", "feat → main", "7 files"]);
  assert.equal(glStatusOf(mr({ changes_count: null }), null, []).changes, null);
  assert.equal(glStatusOf(mr(), null, []).review, "No approvals yet");
});

test("GitLab list status stands in for the pipeline with the merge status", () => {
  const label = (over) => glListStatus(mr(over)).label;
  assert.equal(label({ draft: true, has_conflicts: true }), "Draft");
  assert.equal(label({ has_conflicts: true }), "Conflicts");
  assert.equal(label({ detailed_merge_status: "ci_must_pass" }), "Checks not passing");
  assert.equal(label({ detailed_merge_status: "requested_changes" }), "Changes requested");
  assert.equal(label({ detailed_merge_status: "ci_still_running" }), "Checks running");
  assert.equal(label({ detailed_merge_status: "discussions_not_resolved" }), "Unresolved threads");
  assert.deepEqual([glListStatus(mr({ detailed_merge_status: "mergeable" })).label, glListStatus(mr({ detailed_merge_status: "mergeable" })).tone], ["Ready to merge", "success"]);
  assert.equal(label({ detailed_merge_status: "checking" }), "In review");
  assert.deepEqual(glListStatus(mr()).checks, { passed: 0, failed: 0, running: 0 });
});

test("GitLab list marks the MRs you wrote or review by username, and says the list has no check counts", () => {
  const rows = glListed([mr({ author: { username: "Me", name: "Me M" } }), mr({ reviewers: [{ username: "ME" }] }), mr({ user_notes_count: undefined })], "me", (m) => ({ number: m.iid }));
  assert.deepEqual(rows.map((r) => [r.mine, r.reviewing]), [[true, false], [false, true], [false, false]]);
  assert.deepEqual([rows[0].author, rows[0].head, rows[0].activity, rows[2].activity, rows[0].loaded], ["Me M", "feat", 4, null, false]);
  assert.deepEqual(glWho({ author: { username: "ann" }, reviewers: [{ username: "bob" }, {}] }, "me"), { mine: false, reviewing: false });
});

test("GitLab jobs: manual ones are not run, allowed failures don't block, canceled ones failed", () => {
  const item = (status, extra = {}) => glItem(job(status, extra), 3);
  assert.equal(item("success").state, "passed");
  assert.equal(item("manual").state, "skipped");
  assert.equal(item("skipped").state, "skipped");
  assert.equal(item("running").state, "running");
  for (const s of ["created", "pending", "scheduled", "waiting_for_resource"]) assert.equal(item(s).state, "queued", s);
  const allowed = item("failed", { allow_failure: true });
  assert.deepEqual([allowed.state, allowed.required, allowed.failing], ["passed", false, undefined]);
  const failed = item("failed");
  assert.deepEqual([failed.state, failed.required, failed.kind, failed.failing.project, failed.seconds], ["failed", true, "Job in verify", 3, 61.2]);
  assert.equal(item("canceled").state, "failed");
  assert.equal(item("canceled").note, "Canceled");
});

test("GitLab reviewers are the approvals, by name else username", () => {
  assert.deepEqual(glReviewers({ approved_by: [{ user: { name: "Ann", username: "ann" } }, { user: { username: "bob" } }, {}] }), [{ name: "Ann", vote: "approved" }, { name: "bob", vote: "approved" }, { name: "unknown", vote: "approved" }]);
  assert.deepEqual(glReviewers(null), []);
});

test("GitLab discussions: a diff note has its file and line, replies sit under the first note, and system notes are left out unless asked for", () => {
  const discussions = [
    { id: "d1", notes: [
      { id: 1, type: "DiffNote", resolvable: true, resolved: false, position: { new_path: "a.ts", old_path: "a.ts", new_line: 12, old_line: null }, author: { name: "Ann" }, body: "why?", created_at: "t1" },
      { id: 2, author: { username: "bob" }, body: "because", created_at: "t2" },
    ] },
    { id: "d2", notes: [{ id: 3, system: true, author: { username: "gitlab" }, body: "added 2 commits", created_at: "t3" }] },
    { id: "d3", notes: [{ id: 4, type: "DiffNote", resolvable: true, resolved: true, position: { old_path: "b.ts", new_line: null, old_line: 3 }, author: { name: "Cy" }, body: "gone", created_at: "t4" }] },
  ];
  const url = "https://gitlab.example.com/g/p/-/merge_requests/5";
  const plain = glComments(discussions, url);
  assert.deepEqual(plain.map((c) => [c.id, c.replyTo, c.path, c.line, c.side, c.canReply, c.thread?.label]), [
    [1, null, "a.ts", 12, "additions", true, "Unresolved"],
    [2, 1, null, null, "deletions", false, undefined],
    [4, null, "b.ts", 3, "deletions", true, "Resolved"],
  ]);
  assert.deepEqual([plain[0].discussion, plain[0].author, plain[1].author, plain[0].url], ["d1", "Ann", "bob", url]);
  const withSystem = glComments(discussions, url, true);
  const note = withSystem.find((c) => c.id === 3);
  assert.deepEqual([withSystem.length, note.system, note.canReply], [4, true, false]);
});

test("a GitLab reply keeps the thread's file and discussion, and is a reply to its first comment", () => {
  const root = { id: 1, replyTo: null, path: "a.ts", line: 12, side: "additions", author: "Ann", body: "why?", createdAt: "t1", url: "u", discussion: "d1", canReply: true };
  const reply = glReplied(root, { id: 9, author: { username: "me" }, body: "because", created_at: "t9" }, "because");
  assert.deepEqual([reply.id, reply.replyTo, reply.path, reply.discussion, reply.author, reply.canReply], [9, 1, "a.ts", "d1", "me", false]);
  assert.equal(glReplied(root, { id: 10, created_at: "t" }, "typed").body, "typed");
  assert.equal(glReplied(root, { id: 10, created_at: "t" }, "typed").author, "you");
});
