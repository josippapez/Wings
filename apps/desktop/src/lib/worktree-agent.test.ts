/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { agentInput, baseChoices, clearWorktreeAgentRequest, nestWorktrees, requestWorktreeAgent, suggestBranch, worktreeAgentStore } from "./worktree-agent.ts";

test("the agent is started by typing its command; a shell is left alone", () => {
  assert.equal(agentInput("claude"), "claude\r");
  assert.equal(agentInput("opencode"), "opencode\r");
  assert.equal(agentInput("shell"), null);
});

test("the suggested branch is wings/ and four characters, and changes with the dice", () => {
  assert.match(suggestBranch(), /^wings\/[a-z0-9]{4}$/);
  assert.equal(suggestBranch(() => 0), "wings/aaaa");
  assert.equal(suggestBranch(() => 0.999), "wings/9999");
});

test("the default branch leads the base list once", () => {
  assert.deepEqual(baseChoices({ defaultBranch: "origin/main", branches: ["feature", "origin/main", "main"] }), ["origin/main", "feature", "main"]);
  assert.deepEqual(baseChoices({ defaultBranch: null, branches: ["a"] }), ["a"]);
});

test("worktrees sit under their project, and one whose project is gone becomes a project", () => {
  const s = (id: string, parent?: string) => ({ id, parent });
  const list = [s("a"), s("a-wt", "a"), s("b"), s("orphan", "gone"), s("b-wt", "b"), s("a-wt2", "a")];
  assert.deepEqual(
    nestWorktrees(list).map((r) => `${r.child ? "  " : ""}${r.space.id}`),
    ["a", "  a-wt", "  a-wt2", "b", "  b-wt", "orphan"],
  );
  assert.deepEqual(nestWorktrees([s("x", "x")]).map((r) => r.child), [false]);
});

test("requesting the dialog tells subscribers which project", () => {
  let calls = 0;
  const off = worktreeAgentStore.subscribe(() => calls++);
  requestWorktreeAgent("/code/app");
  assert.equal(worktreeAgentStore.get(), "/code/app");
  clearWorktreeAgentRequest();
  assert.equal(worktreeAgentStore.get(), null);
  assert.equal(calls, 2);
  off();
});
