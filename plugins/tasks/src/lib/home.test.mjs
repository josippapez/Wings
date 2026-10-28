import test from "node:test";
import assert from "node:assert/strict";
import { homeIn } from "./home.ts";

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
