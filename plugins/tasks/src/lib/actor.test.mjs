import test from "node:test";
import assert from "node:assert/strict";
import { actorName, callerOf } from "./actor.ts";

test("callerOf maps a tool call's context to its project and agent session, with the agent's kind", async () => {
  const project = { id: "proj1", name: "web", path: "/work/web" };
  assert.deepEqual(await callerOf({ paneId: "p1", project, session: { id: "s-1", name: "fix totals" } }), { projectId: "proj1", projectName: "web", actor: { id: "s-1", name: "fix totals" } });
  assert.deepEqual((await callerOf({ paneId: "p2", project, session: { id: "s-2", name: null } })).actor, { id: "s-2" });
  assert.deepEqual((await callerOf({ paneId: "p4", project, session: { id: "ses_abc12345", name: null, kind: "opencode" } })).actor, { id: "ses_abc12345", kind: "opencode" });
  // An unnamed session is called by its agent; one from before kinds is Claude Code's.
  assert.equal(actorName({ id: "ses_abc12345", kind: "opencode" }), "OpenCode");
  assert.equal(actorName({ id: "s-2" }), "Claude Code");
  assert.deepEqual((await callerOf({ paneId: "p3", project, session: null })).actor, { id: "" });
  await assert.rejects(callerOf({ paneId: null, project: null, session: null }), /Can't tell which project/);
});

test("an agent goes by its session's latest name, so one session reads as one author after a restart", () => {
  const sessions = [{ id: "c6", name: "Azure DevOps work item 189501", seen: 3 }];
  assert.equal(actorName({ id: "c6", name: "sciensus-digital-core-be-33" }, sessions), "Azure DevOps work item 189501");
  assert.equal(actorName({ id: "other", name: "old name" }, sessions), "old name");
  assert.equal(actorName("you", sessions), "You");
});
