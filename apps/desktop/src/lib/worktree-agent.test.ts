/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { agentInput, baseChoices, clearWorktreeAgentRequest, nestWorktrees, requestWorktreeAgent, suggestBranch, withSetup, worktreeAgentStore, worktreeTitle } from "./worktree-agent.ts";

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

test("a worktree whose folder only repeats its branch is titled by the branch alone", () => {
  assert.deepEqual(worktreeTitle("feature-190597-search", "feature/190597-search"), { title: "feature/190597-search", subtitle: null });
  assert.deepEqual(worktreeTitle("wings-fix-login", "wings/fix-login"), { title: "wings/fix-login", subtitle: null });
});

test("a worktree renamed or on another branch keeps its name and shows the branch", () => {
  assert.deepEqual(worktreeTitle("my-checkout", "feature/x"), { title: "my-checkout", subtitle: "feature/x" });
  assert.deepEqual(worktreeTitle("my-checkout", null), { title: "my-checkout", subtitle: null });
});

test("a project's setup line runs first and the agent starts only if it succeeded", () => {
  assert.equal(withSetup("pnpm install", "claude\r"), "sh -c 'pnpm install' && claude\r");
  assert.equal(withSetup("make bootstrap && pnpm i || true", "claude\r"), "sh -c 'make bootstrap && pnpm i || true' && claude\r");
  assert.equal(withSetup("./scripts/setup.sh", null), "sh -c ./scripts/setup.sh\r");
  assert.equal(withSetup(undefined, "claude\r"), "claude\r");
  assert.equal(withSetup("  ", null), null);
  // Typed without Enter: setup would run and the command wouldn't, so it is left alone.
  assert.equal(withSetup("pnpm install", "claude"), "claude");
});
