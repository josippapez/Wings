import assert from "node:assert/strict";
import test from "node:test";
import { azureStatus } from "./forges/azure.ts";
import { AZURE_VOTES, reviewable, voteArgs } from "./vote.ts";

test("builds the set-vote command for each vote Azure offers", () => {
  for (const vote of Object.keys(AZURE_VOTES)) {
    assert.deepEqual(voteArgs(42, "https://dev.azure.com/acme", vote), ["repos", "pr", "set-vote", "--id", "42", "--vote", vote, "--organization", "https://dev.azure.com/acme", "-o", "json"]);
  }
  assert.equal(voteArgs(1, "https://acme.visualstudio.com", "reset")[8], "https://acme.visualstudio.com");
});

test("refuses a PR id, organization or vote that isn't what the tracker parsed", () => {
  for (const id of [0, -1, 1.5, NaN, Infinity]) assert.throws(() => voteArgs(id, "https://dev.azure.com/acme", "approve"), /pull request id/);
  for (const org of ["http://dev.azure.com/acme", "https://dev.azure.com/acme/", "https://dev.azure.com.evil.net/acme", "https://evil.example/dev.azure.com/acme", "https://dev.azure.com/a/b", "@/etc/passwd", ""]) {
    assert.throws(() => voteArgs(1, org, "approve"), /organization/, org);
  }
  for (const vote of ["APPROVE", "10", "--vote", "@file", "constructor", "__proto__", ""]) assert.throws(() => voteArgs(1, "https://dev.azure.com/acme", vote), /vote/, vote);
});

test("Approve and the Vote menu are only for an open PR: an abandoned Azure DevOps PR gets neither", () => {
  const none = { passed: 0, failed: 0, running: 0 };
  const azure = (status) => azureStatus({ pullRequestId: 7, status, title: "Retry the sync", reviewers: [], creationDate: "2026-10-01T09:00:00Z" }, none);
  assert.equal(reviewable(azure("active")), true);
  assert.equal(reviewable(azure("abandoned")), false);
  assert.equal(reviewable(azure("completed")), false);
  assert.equal(reviewable(undefined), false);
});
