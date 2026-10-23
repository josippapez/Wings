import test from "node:test";
import assert from "node:assert/strict";
import { childrenNote, closedNote, prOps, prStateCommand, prStateOf } from "./pr-state.ts";

const ado = "https://dev.azure.com/Org/Some%20Project/_git/Repo/pullrequest/6762";
const gh = "https://github.com/owner/repo/pull/50";

test("a GitHub or Azure DevOps PR link becomes the CLI call that prints its state; anything else is skipped", () => {
  assert.deepEqual(prStateCommand(gh), { program: "gh", args: ["pr", "view", gh, "--json", "state", "--jq", ".state"] });
  assert.deepEqual(prStateCommand(ado), { program: "az", args: ["repos", "pr", "show", "--id", "6762", "--organization", "https://dev.azure.com/Org", "--query", "status", "--output", "tsv"] });
  for (const other of ["https://gitlab.com/a/b/-/merge_requests/3", "https://github.com/owner/repo/pull/50/files", "not a url"]) assert.equal(prStateCommand(other), null, other);
});

test("gh and az words read as merged, closed or open", () => {
  assert.deepEqual(["MERGED\n", "completed\n", "CLOSED", "abandoned", "OPEN", "active", "draft", ""].map(prStateOf), ["merged", "merged", "closed", "closed", "open", "open", null, null]);
});

const issue = (status, log = []) => ({ key: "SCI-7", title: "t", status, type: "task", log, sessions: [], links: { pr: ado } });

test("a merged PR finishes a live issue with a note saying why; done, to-do and open PRs are left alone", () => {
  assert.deepEqual(prOps(issue("in_review"), "merged", []), [{ kind: "note", key: "SCI-7", text: `The linked pull request was merged: ${ado}.` }, { kind: "update", key: "SCI-7", status: "done" }]);
  assert.equal(prOps(issue("in_progress"), "merged", []).at(-1).status, "done");
  for (const status of ["todo", "done", "canceled"]) assert.deepEqual(prOps(issue(status), "merged", []), [], status);
  assert.deepEqual(prOps(issue("in_review"), "open", []), []);
  assert.deepEqual(prOps({ ...issue("in_review"), links: {} }, "merged", []), []);
});

test("a PR closed without merging leaves a note once and keeps the status", () => {
  assert.deepEqual(prOps(issue("in_review"), "closed", []), [{ kind: "note", key: "SCI-7", text: closedNote(ado) }]);
  assert.deepEqual(prOps(issue("in_review", [{ at: 1, by: "git", kind: "note", text: closedNote(ado) }]), "closed", []), []);
});

test("a merged PR leaves an issue with open children open, with a note once; finished children don't hold it", () => {
  const epic = { ...issue("in_review"), type: "epic" };
  const child = (status) => ({ ...issue(status), key: "SCI-26", parent: "SCI-7", links: {} });
  assert.deepEqual(prOps(epic, "merged", [epic, child("todo")]), [{ kind: "note", key: "SCI-7", text: childrenNote(ado) }]);
  assert.deepEqual(prOps({ ...epic, log: [{ at: 1, by: "git", kind: "note", text: childrenNote(ado) }] }, "merged", [epic, child("todo")]), []);
  assert.equal(prOps(epic, "merged", [epic, child("done"), child("canceled")]).at(-1).status, "done");
});
