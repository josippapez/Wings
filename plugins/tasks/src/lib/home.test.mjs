import test from "node:test";
import assert from "node:assert/strict";
import { homeIn, pickProject } from "./home.ts";

const web = { id: "p1", name: "acme-web" };
const tree = { id: "p2", name: "acme-web-fix", parentId: "p1" };

test("a worktree Wings made keeps its issues in its main project", () => {
  assert.deepEqual(homeIn(tree, [web, tree]), { id: "p1", name: "acme-web" });
});

test("a plain project is its own home", () => {
  assert.deepEqual(homeIn(web, [web, tree]), { id: "p1", name: "acme-web" });
});

test("a worktree whose main project is gone keeps its own", () => {
  assert.deepEqual(homeIn(tree, [tree]), { id: "p2", name: "acme-web-fix" });
});

test("a project to read is picked by path, id or name, never a worktree by name", () => {
  const projects = [
    { id: "a", name: "acme-web", path: "/code/acme-web" },
    { id: "b", name: "acme-api", path: "/code/acme-api" },
    { id: "w", name: "acme-web", path: "/code/acme-web-worktrees/fix", parentId: "a" },
  ];
  assert.equal(pickProject(projects, "/code/acme-api/").id, "b");
  assert.equal(pickProject(projects, "ACME-WEB").id, "a");
  assert.equal(pickProject(projects, "w").id, "w");
  assert.throws(() => pickProject(projects, "nope"), /No project "nope".*acme-web, acme-api/);
});
