import assert from "node:assert/strict";
import test from "node:test";
import { CliError } from "wings-plugin-sdk/cli";
import { counts, encodeQuery, labelOf, ladder, noLog, openOrNewest, orElse, plainFailure, same, withSecondPage } from "./shared.ts";

const idle = { failed: 0, running: 0 };

test("a query value stays in the normal form wings.fetch needs", () => {
  for (const value of [`source.branch.name="feature/it's (x)!*"`, "a b&c=d", "ü/é", "plain-._~"]) {
    const url = `https://api.bitbucket.org/2.0/repositories/w/r/pullrequests?q=${encodeQuery(value)}&state=OPEN`;
    assert.equal(new URL(url).href, url, value);
    assert.equal(decodeURIComponent(new URL(url).searchParams.get("q") ? encodeQuery(value) : ""), value);
  }
});

test("the label of a PR is the first thing that is true, from merged down to approved", () => {
  const label = (facts, words) => labelOf({ checks: idle, ...facts }, words)[0];
  assert.equal(label({ merged: true, closed: true, draft: true, conflicts: true }), "Merged");
  assert.equal(label({ closed: true, draft: true }), "Closed");
  assert.equal(label({ draft: true, conflicts: true }), "Draft");
  assert.equal(label({ conflicts: true, rejected: true, checks: { failed: 1, running: 0 } }), "Conflicts");
  assert.equal(label({ rejected: true, checks: { failed: 1, running: 0 } }), "Rejected");
  assert.equal(label({ checks: { failed: 1, running: 1 }, changes: true }), "Checks failing");
  assert.equal(label({ changes: true, checks: { failed: 0, running: 2 } }), "Changes requested");
  assert.equal(label({ checks: { failed: 0, running: 2 }, unresolved: true }), "Checks running");
  assert.equal(label({ unresolved: true, approved: true }), "Unresolved threads");
  assert.equal(label({ approved: true, reviewing: true }), "Approved");
  assert.equal(label({ reviewing: true }), "In review");
  assert.equal(label({}), "In review");
});

test("a provider can word closed and changes its own way, and what an open PR with nothing to say is called", () => {
  assert.deepEqual(labelOf({ checks: idle, closed: true }, { closed: "Abandoned" }), ["Abandoned", "neutral", "pr-closed"]);
  assert.deepEqual(labelOf({ checks: idle, changes: true }, { changes: "Waiting for author" }), ["Waiting for author", "danger", "pr-open"]);
  assert.deepEqual(labelOf({ checks: idle }, { rest: "Open" }), ["Open", "info", "pr-open"]);
  assert.deepEqual(labelOf({ checks: idle, reviewing: true }, { rest: "Open" }), ["In review", "info", "pr-open"]);
  assert.deepEqual(labelOf({ checks: idle, merged: true }), ["Merged", "merged", "pr-merged"]);
});

test("a ladder of its own keeps the first step that holds, and the icon open unless a step names one", () => {
  assert.deepEqual(ladder([[false, "A", "info"], [1, "B", "danger"], [true, "C", "success"]]), ["B", "danger", "pr-open"]);
  assert.deepEqual(ladder([[true, "Draft", "neutral", "pr-draft"]]), ["Draft", "neutral", "pr-draft"]);
  assert.deepEqual(ladder([]), ["In review", "info", "pr-open"]);
});

test("the open PR of a branch wins over a newer one, else the newest, which a provider lists first", () => {
  const prs = [{ n: 3, open: false }, { n: 2, open: true }, { n: 1, open: false }];
  assert.equal(openOrNewest(prs, (p) => p.open).n, 2);
  assert.equal(openOrNewest([{ n: 9, open: false }, { n: 8, open: false }], (p) => p.open).n, 9);
  assert.equal(openOrNewest([], () => true), undefined);
});

test("a list that fills its page gets the second page too, and a short one doesn't ask", async () => {
  let asked = 0;
  const second = async () => (asked++, ["c"]);
  assert.deepEqual(await withSecondPage(["a", "b"], 2, second), ["a", "b", "c"]);
  assert.deepEqual(await withSecondPage(["a"], 2, second), ["a"]);
  assert.equal(asked, 1);
});

test("a failed check with no log has its summary and link, and Failed when nothing says why", async () => {
  assert.deepEqual(noLog("build", "Timed out", "https://ci/1"), [{ check: "build", summary: "Timed out", log: "", url: "https://ci/1" }]);
  assert.deepEqual(await plainFailure({}, { id: 1, name: "build", summary: null, url: "https://ci/1" }), [{ check: "build", summary: "Failed", log: "", url: "https://ci/1" }]);
  assert.equal((await plainFailure({}, { id: 1, name: "build", summary: "Timed out" }))[0].summary, "Timed out");
});

test("a read that only adds to what is shown gives its fallback when the CLI ran and failed, and throws for a CLI that isn't there", async () => {
  assert.deepEqual(await orElse(Promise.resolve(["a"]), []), ["a"]);
  assert.deepEqual(await orElse(Promise.reject(new CliError("failed", "boom", "boom")), []), []);
  assert.deepEqual(await orElse(Promise.reject(new CliError("signed-out", "login", "login")), []), []);
  await assert.rejects(orElse(Promise.reject(new CliError("missing", "gh is not installed or not on PATH")), []), { kind: "missing" });
  await assert.rejects(orElse(Promise.reject(new SyntaxError("not JSON")), []), SyntaxError);
});

test("counts tally results, and a name matches whatever its case", () => {
  assert.deepEqual(counts(["passed", "failed", "passed", "running"]), { passed: 2, failed: 1, running: 1 });
  assert.equal(same("Ada", "ada"), true);
  assert.equal(same(undefined, "ada"), false);
  assert.equal(same("", ""), false, "no name is nobody");
});
