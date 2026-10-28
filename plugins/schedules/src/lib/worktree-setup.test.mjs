import assert from "node:assert/strict";
import { test } from "node:test";
import { includeText, worktreeChange } from "./worktree-setup.ts";

const defaults = [".env", ".mcp.json"];
const base = { include: null, defaults, hasIncludeFile: false, setup: null };

test("an untouched form changes nothing, and starts from the defaults when the project has no list", () => {
  assert.equal(includeText(base), ".env\n.mcp.json");
  assert.equal(worktreeChange(base, ".env\n.mcp.json\n", ""), undefined);
  assert.equal(worktreeChange({ ...base, include: ["a"], setup: "make" }, "a", " make "), undefined);
});

test("a changed list or line is saved, and only that one", () => {
  assert.deepEqual(worktreeChange(base, ".env\n!big/", ""), { include: [".env", "!big/"] });
  assert.deepEqual(worktreeChange(base, ".env\n.mcp.json", "pnpm install"), { setup: "pnpm install" });
});

test("the default list given back is saved as null, and an emptied setup clears it", () => {
  assert.deepEqual(worktreeChange({ ...base, include: ["a"], setup: "make" }, ".env\n.mcp.json", ""), { include: null, setup: null });
});
