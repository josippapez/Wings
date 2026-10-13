import test from "node:test";
import assert from "node:assert/strict";
import { resolveProject } from "./resolve.ts";

const pane = (paneId, cwd, projectId, project = projectId) => ({ paneId, cwd, projectId, project });
const panes = [pane("home", "/Users/me", "home"), pane("a", "/Users/me/code/a", "a"), pane("a2", "/Users/me/code/a/src", "a"), pane("b", "/Users/me/code/b", "b")];

test("the calling pane's project, with no folder", () => {
  assert.equal(resolveProject(panes, null, "b").id, "b");
  assert.throws(() => resolveProject(panes, null, "nope"), /Can't tell/);
});

test("a folder never matches a terminal above it", () => {
  assert.throws(() => resolveProject(panes, "/Users/me/code/c", null), /No terminal/);
  assert.throws(() => resolveProject([pane("home", "/Users/me", "home")], "/Users/me/code/a", null), /No terminal/);
});

test("a folder matches a terminal in it or below it", () => {
  assert.equal(resolveProject(panes, "/Users/me/code/a/", null).id, "a");
  assert.equal(resolveProject(panes, "/Users/me/code/a/src", null).id, "a");
});

test("a folder over several projects is ambiguous unless one terminal is exactly in it", () => {
  assert.throws(() => resolveProject(panes, "/Users/me/code", null), /several projects/);
  assert.equal(resolveProject([...panes, pane("c", "/Users/me/code", "code")], "/Users/me/code", null).id, "code");
});
