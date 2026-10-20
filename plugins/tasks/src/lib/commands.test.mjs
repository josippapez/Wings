import test from "node:test";
import assert from "node:assert/strict";
import { AGENT_KINDS, agentCommand, PLAIN_WORD, resumeCommand } from "./commands.ts";
import { findIssue } from "./orderings.ts";
import { create, fresh } from "./testing.mjs";

test("resume and start commands are plain words Wings accepts, and refuse anything else", () => {
  const id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
  // A session recorded before sessions had a kind is Claude Code's.
  assert.equal(resumeCommand({ id }), `claude --resume ${id}`);
  for (const word of resumeCommand({ id }).split(" ")) assert.match(word, PLAIN_WORD);
  // The prompt goes as one openPane arg, a sentence Wings quotes; it holds nothing a shell or Claude reads as a flag.
  const start = agentCommand({ key: "WEB-12" });
  assert.equal(start.command, "claude");
  assert.equal(start.args.length, 1);
  assert.match(start.args[0], /^Continue issue WEB-12\. Read it with tasks__get/);
  assert.ok(!start.args[0].startsWith("-") && !/[\x00-\x1f'`$]/.test(start.args[0]));
  for (const bad of ["x; rm -rf ~", "$(id)", "a b", "", "../x", "id'quote"]) assert.throws(() => resumeCommand({ id: bad }), /session id/);
  for (const bad of ["WEB-1; ls", "web-1", "WEB", "WEB-1 x", "$(id)-1", "WEBS-1"]) assert.throws(() => agentCommand({ key: bad }), /issue key/);
});

test("start agent names the issue by the key it has, however the tab spelled the key it was opened with", () => {
  const { store } = create(fresh(), "A");
  assert.match(agentCommand(findIssue(store, "web-1")).args[0], /^Continue issue WEB-1\. /);
});

test("each agent resumes its own sessions and starts with the prompt as one arg", () => {
  assert.equal(resumeCommand({ id: "ses_abc12345XYZ", kind: "opencode" }), "opencode --session ses_abc12345XYZ");
  assert.equal(resumeCommand({ id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", kind: "claude" }), "claude --resume 6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11");
  // A session id of one agent isn't taken for another's.
  assert.throws(() => resumeCommand({ id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", kind: "opencode" }), /OpenCode session id/);
  assert.throws(() => resumeCommand({ id: "ses_x; ls", kind: "opencode" }), /session id/);
  const start = agentCommand({ key: "WEB-12" }, "opencode");
  assert.equal(start.command, "opencode --prompt");
  assert.match(start.args[0], /^Continue issue WEB-12\. /);
  for (const kind of AGENT_KINDS) for (const word of agentCommand({ key: "WEB-12" }, kind).command.split(" ")) assert.match(word, PLAIN_WORD);
});
