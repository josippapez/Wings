import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("the manifest: tools tell agents to link by hand and keep keys out of git, and git is only read for a file diff", async () => {
  const m = JSON.parse(readFileSync(new URL("../wings-plugin.json", import.meta.url), "utf8"));
  const text = Object.fromEntries(m.contributes.mcpTools.map((t) => [t.name, t.description]));
  for (const name of ["create", "update", "log"]) assert.match(text[name], /Issue keys are local to Wings: never put them in commit messages, branch names, pull request titles or descriptions/);
  assert.match(text.update, /set commit only for one you made in another repo/);
  assert.match(text.update, /set pr to its URL/);
  assert.match(text.update, /set branch when you start on a branch/);
  for (const t of m.contributes.mcpTools) assert.doesNotMatch(t.description, /put the key in your commit/);
  assert.deepEqual(Object.keys(m.permissions).sort(), ["events", "exec", "openUrl", "panes"]);
  // A linked pull request's state, the PR of a branch, and the diff of a changed file you click; commits come from the journal, never from git.
  assert.deepEqual(m.permissions.exec, ["gh pr view [--json --jq]", "az repos pr show [--id --organization --query --output]", "git diff [-- --no-color --no-ext-diff]", "git remote get-url", "gh pr list [--head --state --limit --json]", "az repos pr list [--organization --project --repository --source-branch --status --top --output]"]);
  assert.deepEqual(m.permissions.events, ["pr-tracker pr-state"]);
});

test("the manifest: the instructions line sends Claude to the tools unprompted, and the settings page is declared", async () => {
  const m = JSON.parse(readFileSync(new URL("../wings-plugin.json", import.meta.url), "utf8"));
  const line = m.contributes.mcpInstructions;
  assert.ok(line.length <= 500);
  for (const name of ["create", "list", "get", "update", "log"]) assert.match(line, new RegExp(`tasks__${name}\\b`));
  assert.match(line, /For work that changes files, even unasked/);
  assert.match(line, /A question, check or review gets no issue/);
  assert.match(line, /deciding something gets no log note/);
  assert.deepEqual(m.contributes.hooks, ["SessionStart", "PreToolUse", "UserPromptSubmit", "Journal"]);
  assert.equal(m.contributes.settings.page, "dist/settings.html");
  assert.ok(m.contributes.mcpTools.find((t) => t.name === "list").inputSchema.properties.query);
});
