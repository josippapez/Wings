import assert from "node:assert/strict";
import test from "node:test";
import { azWho, ghWho, glWho } from "./who.ts";

test("GitHub: you wrote it, or your login is among the review requests (teams have no login)", () => {
  assert.deepEqual(ghWho({ author: { login: "Me" }, reviewRequests: [] }, "me"), { mine: true, reviewing: false });
  assert.deepEqual(ghWho({ author: { login: "ann" }, reviewRequests: [{ login: "bob" }, { login: "ME" }] }, "me"), { mine: false, reviewing: true });
  assert.deepEqual(ghWho({ author: { login: "ann" }, reviewRequests: [{}] }, "me"), { mine: false, reviewing: false });
  assert.deepEqual(ghWho({}, "me"), { mine: false, reviewing: false });
});

test("GitHub: a review request to your team counts through the search's numbers", () => {
  const pr = { number: 7, author: { login: "ann" }, reviewRequests: [{ slug: "core" }] };
  assert.equal(ghWho(pr, "me").reviewing, false);
  assert.equal(ghWho(pr, "me", new Set([7])).reviewing, true);
  assert.equal(ghWho(pr, "me", new Set([8])).reviewing, false);
});

test("GitLab: author and reviewers by username", () => {
  assert.deepEqual(glWho({ author: { username: "me" }, reviewers: [] }, "me"), { mine: true, reviewing: false });
  assert.deepEqual(glWho({ author: { username: "ann" }, reviewers: [{ username: "me" }] }, "me"), { mine: false, reviewing: true });
  assert.deepEqual(glWho({ author: { username: "ann" } }, "me"), { mine: false, reviewing: false });
});

test("Azure DevOps: creator and reviewers by unique name, without case", () => {
  assert.deepEqual(azWho({ createdBy: { uniqueName: "Me@Example.com" }, reviewers: [] }, "me@example.com"), { mine: true, reviewing: false });
  assert.deepEqual(azWho({ createdBy: { uniqueName: "ann@example.com" }, reviewers: [{ uniqueName: "bob@example.com" }, { uniqueName: "ME@example.com" }] }, "me@example.com"), { mine: false, reviewing: true });
  assert.deepEqual(azWho({ createdBy: {}, reviewers: [{}] }, "me@example.com"), { mine: false, reviewing: false });
});
