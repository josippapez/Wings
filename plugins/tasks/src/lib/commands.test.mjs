import test from "node:test";
import assert from "node:assert/strict";
import { agentCommand, PLAIN_WORD, resumeCommand } from "./commands.ts";
import { findIssue } from "./orderings.ts";
import { create, fresh } from "./testing.mjs";

test("resume and start commands are plain words Wings accepts, and refuse anything else", () => {
  const id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
  assert.equal(resumeCommand(id), `claude --resume ${id}`);
  for (const word of resumeCommand(id).split(" ")) assert.match(word, PLAIN_WORD);
  // The prompt goes as one openPane arg, a sentence Wings quotes; it holds nothing a shell or Claude reads as a flag.
  const start = agentCommand({ key: "WEB-12" });
  assert.equal(start.command, "claude");
  assert.equal(start.args.length, 1);
  assert.match(start.args[0], /^Continue issue WEB-12\. Read it with tasks__get/);
  assert.ok(!start.args[0].startsWith("-") && !/[\x00-\x1f'`$]/.test(start.args[0]));
  for (const bad of ["x; rm -rf ~", "$(id)", "a b", "", "../x", "id'quote"]) assert.throws(() => resumeCommand(bad), /session id/);
  for (const bad of ["WEB-1; ls", "web-1", "WEB", "WEB-1 x", "$(id)-1", "WEBS-1"]) assert.throws(() => agentCommand({ key: bad }), /issue key/);
});

test("start agent names the issue by the key it has, however the tab spelled the key it was opened with", () => {
  const { store } = create(fresh(), "A");
  assert.match(agentCommand(findIssue(store, "web-1")).args[0], /^Continue issue WEB-1\. /);
});
