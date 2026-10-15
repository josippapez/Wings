import assert from "node:assert/strict";
import test from "node:test";
import { bbComments, bbItem, bbListed, bbOwn, bbStatusOf, bbWho, LIST_FIELDS } from "./bitbucket.ts";

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

test("Bitbucket statuses are checks: builds, stopped ones, and a state it doesn't name", () => {
  assert.equal(bbItem({ key: "k", type: "build", state: "SUCCESSFUL", name: "Build", updated_on: "2026-01-01T00:00:00Z" }).state, "passed");
  assert.equal(bbItem({ key: "k", state: "INPROGRESS" }).state, "running");
  const stopped = bbItem({ key: "k", state: "STOPPED" });
  assert.deepEqual([stopped.state, stopped.note, stopped.kind], ["failed", "Stopped", "Status check"]);
  assert.equal(bbItem({ key: "k", type: "build", state: "FAILED" }).kind, "Build");
});

test("Bitbucket list keeps every open PR and marks the ones you wrote or review", () => {
  const prs = [
    bb({ id: 1, author: { uuid: "{other}" } }),
    bb({ id: 2, author: { uuid: "{other}" }, participants: [{ role: "REVIEWER", user: { uuid: "{me}" } }] }),
    bb({ id: 3, author: { uuid: "{other}" }, reviewers: [{ uuid: "{me}" }] }),
    bb({ id: 4 }),
    bb({ id: 5, author: { uuid: "{other}" }, participants: [{ role: "PARTICIPANT", user: { uuid: "{me}" } }] }),
  ];
  assert.deepEqual(bbWho(prs[1], "{me}"), { mine: false, reviewing: true });
  const rows = bbListed(prs, "{me}", (pr) => ({ number: pr.id }));
  assert.deepEqual(rows.map((r) => [r.link.number, r.mine, r.reviewing]), [[1, false, false], [2, false, true], [3, false, true], [4, true, false], [5, false, false]]);
  assert.equal(rows[3].activity, 3);
  assert.equal(rows[3].head, "feat");
  assert.equal(rows[3].loaded, false, "the list has no check results");
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

test("the list asks for reviewers and participants, which the API leaves out by default, in normal form", () => {
  const url = `https://api.bitbucket.org/2.0/repositories/w/r/pullrequests?state=OPEN&sort=-updated_on&pagelen=50&${LIST_FIELDS}`;
  assert.equal(new URL(url).href, url);
  assert.equal(new URL(url).searchParams.get("fields"), "+values.participants,+values.reviewers");
});

test("a branch lookup keeps only PRs from this repository, not a fork's branch of the same name", () => {
  const prs = [bb({ id: 1, source: { branch: { name: "main" }, repository: { full_name: "someone/r" } } }), bb({ id: 2, source: { branch: { name: "main" }, repository: { full_name: "W/R" } } }), bb({ id: 3, source: { branch: { name: "main" }, repository: null } })];
  assert.deepEqual(bbOwn(prs, "w/r").map((p) => p.id), [2]);
});
