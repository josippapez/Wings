import test from "node:test";
import assert from "node:assert/strict";
import { resolvePane } from "./actor.ts";

test("resolvePane maps a tool call's paneId to its project and Claude session", () => {
  const panes = [
    { paneId: "p1", project: "web", projectId: "proj1", session: { sessionId: "s-1", name: "fix totals" } },
    { paneId: "p2", project: "api", projectId: "proj2", session: { sessionId: "s-2", name: null } },
    { paneId: "p3", project: "api", projectId: "proj2", session: null },
  ];
  assert.deepEqual(resolvePane(panes, "p1"), { projectId: "proj1", projectName: "web", actor: { id: "s-1", name: "fix totals" } });
  assert.deepEqual(resolvePane(panes, "p2").actor, { id: "s-2" });
  assert.deepEqual(resolvePane(panes, "p3").actor, { id: "" });
  assert.throws(() => resolvePane(panes, "nope"), /Can't tell which project/);
  assert.throws(() => resolvePane(panes, null), /Can't tell which project/);
});
