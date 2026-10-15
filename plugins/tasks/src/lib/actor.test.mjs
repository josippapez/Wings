import test from "node:test";
import assert from "node:assert/strict";
import { callerOf } from "./actor.ts";

test("callerOf maps a tool call's context to its project and Claude session", () => {
  const project = { id: "proj1", name: "web", path: "/work/web" };
  assert.deepEqual(callerOf({ paneId: "p1", project, session: { id: "s-1", name: "fix totals" } }), { projectId: "proj1", projectName: "web", actor: { id: "s-1", name: "fix totals" } });
  assert.deepEqual(callerOf({ paneId: "p2", project, session: { id: "s-2", name: null } }).actor, { id: "s-2" });
  assert.deepEqual(callerOf({ paneId: "p3", project, session: null }).actor, { id: "" });
  assert.throws(() => callerOf({ paneId: null, project: null, session: null }), /Can't tell which project/);
});
