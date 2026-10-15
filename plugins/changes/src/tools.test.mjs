import assert from "node:assert/strict";
import { test } from "node:test";
import { registerTools } from "wings-plugin-sdk/tools";

import { fakeWings, ok, repos } from "./lib/fake-wings.mjs";
import { tools } from "./tools.ts";

const fake = fakeWings();
const handlers = {};
registerTools(tools, { onTool: (name, handler) => void (handlers[name] = handler) });
const call = (name, input) => Promise.resolve().then(() => handlers[name](input, { paneId: null }));

test("a limit outside 1 to 200, an unknown against and a missing cwd are errors, not clamped or guessed", async () => {
  await assert.rejects(call("recent_commits", { cwd: "/w/a", limit: 500 }), /limit must be at most 200/);
  await assert.rejects(call("recent_commits", { cwd: "/w/a", limit: 0 }), /limit must be at least 1/);
  await assert.rejects(call("changed_files", { cwd: "/w/a", against: "main" }), /against must be one of uncommitted, branch/);
  await assert.rejects(call("changed_files", {}), /cwd is required/);
});

test("recent_commits names a detached checkout by its commit", async () => {
  const log = ["c0ffee".padEnd(40, "0"), "", "Ada", "100", "100", "HEAD", "Fix the thing"].join("\0") + "\0";
  fake.reply = (args, cwd) => (args === "branch --show-current" ? ok("\n") : args.startsWith("log") ? ok(log) : repos()(args, cwd));
  const { branch, commits } = await call("recent_commits", { cwd: "/w/t1" });
  assert.equal(branch, "detached at abc1234");
  assert.equal(commits[0].subject, "Fix the thing");
});

test("a folder outside any repo is an error that names it", async () => {
  fake.reply = repos("/tmp/plain");
  await assert.rejects(call("changed_files", { cwd: "/tmp/plain" }), /\/tmp\/plain is not inside a git repo/);
});
