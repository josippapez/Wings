import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("the manifest: tools tell agents to link by hand and keep keys out of git, and nothing reads git or transcripts", async () => {
  const m = JSON.parse(readFileSync(new URL("../wings-plugin.json", import.meta.url), "utf8"));
  const text = Object.fromEntries(m.contributes.mcpTools.map((t) => [t.name, t.description]));
  for (const name of ["create", "update", "log"]) assert.match(text[name], /Issue keys are local to Wings: never put them in commit messages, branch names, pull request titles or descriptions/);
  assert.match(text.update, /set commit only for one you made in another repo/);
  assert.match(text.update, /set pr to its URL/);
  assert.match(text.update, /set branch when you start on a branch/);
  for (const t of m.contributes.mcpTools) assert.doesNotMatch(t.description, /put the key in your commit/);
  assert.deepEqual(Object.keys(m.permissions).sort(), ["openUrl", "panes"]);
});

test("the manifest: the instructions line sends Claude to the tools unprompted, and the settings page is declared", async () => {
  const m = JSON.parse(readFileSync(new URL("../wings-plugin.json", import.meta.url), "utf8"));
  const line = m.contributes.mcpInstructions;
  assert.ok(line.length <= 500);
  for (const name of ["create", "list", "get", "update", "log", "todo"]) assert.match(line, new RegExp(`tasks__${name}\\b`));
  assert.match(line, /for every piece of work, not only when asked/);
  assert.deepEqual(m.contributes.hooks, ["SessionStart", "PreToolUse", "Journal"]);
  assert.equal(m.contributes.settings.page, "dist/settings.html");
  assert.ok(m.contributes.mcpTools.find((t) => t.name === "list").inputSchema.properties.query);
});
