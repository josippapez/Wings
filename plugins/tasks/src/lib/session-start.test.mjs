import test from "node:test";
import assert from "node:assert/strict";
import { sessionStartText } from "./session-start.ts";

test("the session start line names the linked issue and the steps left, or asks for an issue", () => {
  const issue = { key: "WEB-4", title: "Checkout summary", status: "in_progress" };
  const linked = sessionStartText({ projectName: "web", keepLocal: true, issue, todoLeft: 3 });
  assert.match(linked, /WEB-4 "Checkout summary" \(In progress\)/);
  assert.match(linked, /3 steps left; tasks__todo with no items reads it/);
  const fresh = sessionStartText({ projectName: "web", keepLocal: true });
  assert.match(fresh, /^Do this first, before any other tool call/m);
  assert.match(fresh, /tasks__list or create one with tasks__create/);
  assert.doesNotMatch(fresh, /steps? left/);
  assert.doesNotMatch(sessionStartText({ projectName: "web", keepLocal: false }), /never go in commits/);
});
