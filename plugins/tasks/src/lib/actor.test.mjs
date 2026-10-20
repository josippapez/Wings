import test from "node:test";
import assert from "node:assert/strict";
import { actorName, callerOf } from "./actor.ts";

test("callerOf maps a tool call's context to its project and agent session, with the agent's kind", () => {
  const project = { id: "proj1", name: "web", path: "/work/web" };
  assert.deepEqual(callerOf({ paneId: "p1", project, session: { id: "s-1", name: "fix totals" } }), { projectId: "proj1", projectName: "web", actor: { id: "s-1", name: "fix totals" } });
  assert.deepEqual(callerOf({ paneId: "p2", project, session: { id: "s-2", name: null } }).actor, { id: "s-2" });
  assert.deepEqual(callerOf({ paneId: "p4", project, session: { id: "ses_abc12345", name: null, kind: "opencode" } }).actor, { id: "ses_abc12345", kind: "opencode" });
  // An unnamed session is called by its agent; one from before kinds is Claude Code's.
  assert.equal(actorName({ id: "ses_abc12345", kind: "opencode" }), "OpenCode");
  assert.equal(actorName({ id: "s-2" }), "Claude Code");
  assert.deepEqual(callerOf({ paneId: "p3", project, session: null }).actor, { id: "" });
  assert.throws(() => callerOf({ paneId: null, project: null, session: null }), /Can't tell which project/);
});
