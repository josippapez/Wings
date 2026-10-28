import test from "node:test";
import assert from "node:assert/strict";
import { branchPrCommand, branchPrOf, childrenNote, closedNote, foundOps, isPrOf, prOps, prStateCommand, prStateEventOf, prStateOf } from "./pr-state.ts";

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

const ghRemote = { kind: "github", host: "github.com", path: "owner/repo" };
const adoRemote = { kind: "azure", org: "https://dev.azure.com/Org", project: "Some Project", repo: "Repo" };

test("a branch's PRs are listed with the host's CLI, for github.com and Azure DevOps only, and never for a branch that looks like a flag", () => {
  assert.deepEqual(branchPrCommand(ghRemote, "feature/x"), { program: "gh", args: ["pr", "list", "--head", "feature/x", "--state", "all", "--limit", "5", "--json", "url,state"] });
  assert.equal(branchPrCommand(adoRemote, "feature/x").args.at(-1), "json");
  assert.equal(branchPrCommand(adoRemote, "feature/x").args[branchPrCommand(adoRemote, "feature/x").args.indexOf("--source-branch") + 1], "feature/x");
  assert.equal(branchPrCommand({ kind: "gitlab", host: "gitlab.com", path: "a/b" }, "x"), null);
  assert.equal(branchPrCommand(ghRemote, "--repo=evil"), null);
});

test("the PR of a branch is the open one, else the newest; nothing for no PRs or output that isn't a list", () => {
  const gh2 = JSON.stringify([{ url: "https://github.com/owner/repo/pull/9", state: "MERGED" }, { url: "https://github.com/owner/repo/pull/8", state: "CLOSED" }]);
  assert.deepEqual(branchPrOf(ghRemote, gh2), { url: "https://github.com/owner/repo/pull/9", state: "merged" });
  assert.equal(branchPrOf(ghRemote, JSON.stringify([{ url: "u", state: "CLOSED" }, { url: "v", state: "OPEN" }])).url, "v");
  const az = JSON.stringify([{ pullRequestId: 6762, status: "completed", repository: { webUrl: "https://dev.azure.com/Org/Some%20Project/_git/Repo" } }]);
  assert.deepEqual(branchPrOf(adoRemote, az), { url: ado, state: "merged" });
  assert.equal(branchPrOf(adoRemote, JSON.stringify([{ pullRequestId: 5, status: "active" }])).url, "https://dev.azure.com/Org/Some%20Project/_git/Repo/pullrequest/5");
  for (const out of ["[]", "", "not json", "{}"]) assert.equal(branchPrOf(ghRemote, out), null, out);
});

const onBranch = (status, links = { branch: "feature/x" }) => ({ key: "SCI-7", title: "t", status, type: "task", log: [], sessions: [], links });

test("a merged PR belongs to a live issue by its link, or by its branch when it has no link", () => {
  const pr = { url: gh, sourceBranch: "feature/x" };
  assert.equal(isPrOf(onBranch("in_review"), pr), true);
  assert.equal(isPrOf(onBranch("in_review", { pr: gh }), pr), true);
  assert.equal(isPrOf(onBranch("in_review", { pr: ado, branch: "feature/x" }), pr), false);
  assert.equal(isPrOf(onBranch("in_review", { branch: "other" }), pr), false);
  assert.equal(isPrOf(onBranch("done"), pr), false);
  assert.equal(isPrOf(onBranch("todo"), pr), false);
});

test("finding a PR links it and then does what its state does", () => {
  assert.deepEqual(foundOps(onBranch("in_progress"), { url: gh, state: "merged" }, []), [
    { kind: "update", key: "SCI-7", pr: gh },
    { kind: "note", key: "SCI-7", text: `The linked pull request was merged: ${gh}.` },
    { kind: "update", key: "SCI-7", status: "done" },
  ]);
  assert.deepEqual(foundOps(onBranch("in_progress"), { url: gh, state: "open" }, []), [{ kind: "update", key: "SCI-7", pr: gh }]);
  assert.equal(foundOps(onBranch("in_progress", { pr: gh }), { url: gh, state: "merged" }, [])[0].kind, "note");
});

test("the pr-state event is read from JSON and refused when a field is missing or wrong", () => {
  assert.deepEqual(prStateEventOf({ url: gh, state: "merged", sourceBranch: "b", projectId: "p", extra: 1 }), { url: gh, state: "merged", sourceBranch: "b", projectId: "p" });
  for (const bad of [null, {}, { url: gh, state: "open", sourceBranch: "b", projectId: "p" }, { url: gh, state: "merged", sourceBranch: "", projectId: "p" }]) assert.equal(prStateEventOf(bad), null);
});
