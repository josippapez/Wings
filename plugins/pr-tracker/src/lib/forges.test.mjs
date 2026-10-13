import assert from "node:assert/strict";
import test from "node:test";
import { bbComments, bbListed, bbOwn, bbStatusOf, bbWho, encodeQuery, gtComments, gtListed, gtOwn, gtQuoted, gtStatusOf, LIST_FIELDS, plainFailure } from "./forges.ts";
import { signInKey } from "./state.ts";

test("a query value stays in the normal form wings.fetch needs", () => {
  for (const value of [`source.branch.name="feature/it's (x)!*"`, "a b&c=d", "ü/é", "plain-._~"]) {
    const url = `https://api.bitbucket.org/2.0/repositories/w/r/pullrequests?q=${encodeQuery(value)}&state=OPEN`;
    assert.equal(new URL(url).href, url, value);
    assert.equal(decodeURIComponent(new URL(url).searchParams.get("q") ? encodeQuery(value) : ""), value);
  }
});

const bb = (over = {}) => ({
  id: 7, title: "Add x", state: "OPEN", draft: false, comment_count: 3,
  author: { uuid: "{me}", display_name: "Me" },
  source: { branch: { name: "feat" }, commit: { hash: "abc123" }, repository: { full_name: "w/r" } },
  destination: { branch: { name: "main" } },
  updated_on: "2026-10-01T10:00:00+00:00",
  links: { html: { href: "https://bitbucket.org/w/r/pull-requests/7" } },
  ...over,
});

test("Bitbucket status: state, draft, checks and reviews decide the label", () => {
  assert.equal(bbStatusOf(bb({ state: "MERGED" }), []).label, "Merged");
  assert.equal(bbStatusOf(bb({ state: "DECLINED" }), []).label, "Closed");
  assert.equal(bbStatusOf(bb({ draft: true }), []).label, "Draft");
  const failed = bbStatusOf(bb(), [{ key: "k1", state: "FAILED", name: "Pipeline #1", url: "https://x/1" }, { key: "k2", state: "SUCCESSFUL" }, { key: "k3", state: "INPROGRESS" }]);
  assert.equal(failed.label, "Checks failing");
  assert.deepEqual(failed.checks, { passed: 1, failed: 1, running: 1 });
  assert.deepEqual(failed.failing.map((f) => [f.id, f.name, f.url]), [["k1", "Pipeline #1", "https://x/1"]]);
  assert.equal(bbStatusOf(bb(), [{ key: "k", state: "INPROGRESS" }]).label, "Checks running");
  const approved = bb({ participants: [{ role: "REVIEWER", approved: true, user: { uuid: "{a}" } }] });
  assert.equal(bbStatusOf(approved, []).label, "Approved");
  assert.equal(bbStatusOf(approved, []).review, "1 approved");
  const changes = bb({ participants: [{ role: "REVIEWER", approved: false, state: "changes_requested", user: { uuid: "{a}" } }] });
  assert.equal(bbStatusOf(changes, []).label, "Changes requested");
  const st = bbStatusOf(bb(), []);
  assert.deepEqual([st.key, st.branch, st.base, st.head, st.label], ["abc123", "feat → main", "main", "feat", "In review"]);
});

test("Bitbucket list keeps the PRs you wrote or review, yours first by the list's own order", () => {
  const prs = [
    bb({ id: 1, author: { uuid: "{other}" } }),
    bb({ id: 2, author: { uuid: "{other}" }, participants: [{ role: "REVIEWER", user: { uuid: "{me}" } }] }),
    bb({ id: 3, author: { uuid: "{other}" }, reviewers: [{ uuid: "{me}" }] }),
    bb({ id: 4 }),
    bb({ id: 5, author: { uuid: "{other}" }, participants: [{ role: "PARTICIPANT", user: { uuid: "{me}" } }] }),
  ];
  assert.deepEqual(bbWho(prs[1], "{me}"), { mine: false, reviewing: true });
  const rows = bbListed(prs, "{me}", (pr) => ({ number: pr.id }));
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine]), [[2, false], [3, false], [4, true]]);
  assert.equal(rows[2].activity, 3);
  assert.equal(rows[2].head, "feat");
});

test("Bitbucket comments: inline position, deleted ones dropped, replies under the thread's first comment", () => {
  const url = "https://bitbucket.org/w/r/pull-requests/7";
  const list = bbComments(
    [
      { id: 1, created_on: "t1", content: { raw: "top" }, user: { display_name: "A" }, inline: { path: "src/a.ts", to: 12, from: null } },
      { id: 2, created_on: "t2", content: { raw: "reply" }, user: { display_name: "B" }, parent: { id: 1 }, inline: { path: "src/a.ts", to: 12 } },
      { id: 3, created_on: "t3", content: { raw: "reply to reply" }, user: { display_name: "A" }, parent: { id: 2 } },
      { id: 4, created_on: "t4", content: { raw: "gone" }, deleted: true },
      { id: 5, created_on: "t5", content: { raw: "whole PR" }, user: { display_name: "C" } },
      { id: 6, created_on: "t6", content: { raw: "old line" }, inline: { path: "b.ts", from: 3, to: null } },
    ],
    url,
  );
  assert.deepEqual(list.map((c) => [c.id, c.replyTo, c.path, c.line, c.side]), [
    [1, null, "src/a.ts", 12, "additions"],
    [2, 1, "src/a.ts", 12, "additions"],
    [3, 1, null, null, "additions"],
    [5, null, null, null, "additions"],
    [6, null, "b.ts", 3, "deletions"],
  ]);
  assert.deepEqual(list.map((c) => c.canReply), [true, false, false, true, true], "a thread takes a reply on its first comment");
  assert.equal(list[3].author, "C");
  assert.equal(list[3].url, url);
});

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

test("Gitea list keeps the PRs you wrote or were asked to review, matching the login without case", () => {
  const prs = [gt({ number: 1, user: { login: "other" } }), gt({ number: 2, user: { login: "other" }, requested_reviewers: [{ login: "ME" }] }), gt({ number: 3 }), gt({ number: 4, requested_reviewers: null, user: { login: "x" } })];
  const rows = gtListed(prs, "me", (pr) => ({ number: pr.number }));
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine]), [[2, false], [3, true]]);
  assert.equal(rows[1].activity, 3);
  assert.equal(rows[1].head, "fix");
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

test("a failed check on these forges has no log, only its summary and link", async () => {
  assert.deepEqual(await plainFailure({}, { id: 1, name: "build", summary: null, url: "https://ci/1" }), [{ check: "build", summary: "Failed", log: "", url: "https://ci/1" }]);
});

test("the list asks for reviewers and participants, which the API leaves out by default, in normal form", () => {
  const url = `https://api.bitbucket.org/2.0/repositories/w/r/pullrequests?state=OPEN&sort=-updated_on&pagelen=50&${LIST_FIELDS}`;
  assert.equal(new URL(url).href, url);
  assert.equal(new URL(url).searchParams.get("fields"), "+values.participants,+values.reviewers");
});

test("a branch lookup keeps only PRs from this repository, not a fork's branch of the same name", () => {
  const prs = [bb({ id: 1, source: { branch: { name: "main" }, repository: { full_name: "someone/r" } } }), bb({ id: 2, source: { branch: { name: "main" }, repository: { full_name: "W/R" } } }), bb({ id: 3, source: { branch: { name: "main" }, repository: null } })];
  assert.deepEqual(bbOwn(prs, "w/r").map((p) => p.id), [2]);
  const gts = [gt({ number: 1, head: { ref: "forgejo", sha: "a", repo: { full_name: "fork/r" } } }), gt({ number: 2, head: { ref: "forgejo", sha: "b", repo: { full_name: "O/R" } } }), gt({ number: 3, head: { ref: "forgejo", sha: "c", repo: null } })];
  assert.deepEqual(gtOwn(gts, "o/r").map((p) => p.number), [2]);
});

test("sign-in state is per host for GitHub and GitLab, so github.com and an Enterprise host don't share it", () => {
  assert.notEqual(signInKey({ kind: "github", host: "github.com", path: "o/r" }), signInKey({ kind: "github", host: "ghe.example", path: "o/r" }));
  assert.notEqual(signInKey({ kind: "github", host: "x", path: "" }), signInKey({ kind: "gitlab", host: "x", path: "" }));
  assert.equal(signInKey({ kind: "bitbucket", workspace: "w", repo: "r" }), "bitbucket");
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
