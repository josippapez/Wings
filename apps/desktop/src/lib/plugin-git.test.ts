import assert from "node:assert/strict";
import { test } from "node:test";

import { gitListeners } from "./plugin-git.ts";

const frame = (name: string, exec: string[], ready = true) => ({ name, ready, plugin: { permissions: { exec } } });

test("a plugin with a git exec entry hears it, whatever the subcommand or flag list", () => {
  const frames = [frame("a", ["git"]), frame("b", ["git branch --show-current"]), frame("c", ["git log [--oneline]"]), frame("d", ["gh pr view", "git status"])];
  assert.deepEqual(gitListeners(frames).map((f) => f.name), ["a", "b", "c", "d"]);
});

test("a plugin without git, or with a program that only starts with it, does not", () => {
  const frames = [frame("a", ["gh pr view"]), frame("b", ["gitx"]), frame("c", []), frame("d", ["az boards"])];
  assert.deepEqual(gitListeners(frames), []);
});

test("main and page frames of the plugin both hear it, but a frame that isn't ready doesn't", () => {
  const frames = [frame("main", ["git"]), frame("page", ["git"]), frame("starting", ["git"], false)];
  assert.deepEqual(gitListeners(frames).map((f) => f.name), ["main", "page"]);
});
