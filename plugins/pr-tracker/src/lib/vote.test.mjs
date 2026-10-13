import assert from "node:assert/strict";
import test from "node:test";
import { AZURE_VOTES, voteArgs } from "./vote.ts";

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
