/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView, ToolStatus } from "./api.ts";
import { onToolInstall, pluginsUsing, requestToolInstall, settleInstalls, type InstallWatch } from "./tool-install.ts";

const tool: ToolStatus = { program: "az", name: "Azure CLI", installed: false, command: "brew update && brew install azure-cli", needsHomebrew: false, docs: "https://learn.microsoft.com/cli/azure/install-azure-cli" };
const watch = (over: Partial<InstallWatch> = {}): InstallWatch => ({ pluginId: "work-item", pluginName: "Work item", tool, paneId: "p1", seen: false, ran: false, ...over });

test("an install is over once brew ran and the shell prompt is back", () => {
  let watches = [watch()];
  // The tab's shell hasn't started yet.
  let step = settleInstalls(watches, {});
  assert.equal(step.done.length, 0);
  watches = step.running;
  step = settleInstalls(watches, { p1: { command: "zsh", cwd: "/w", busy: false } });
  assert.equal(step.done.length, 0, "the typed line hasn't started");
  watches = step.running;
  step = settleInstalls(watches, { p1: { command: "brew", cwd: "/w", busy: true } });
  assert.equal(step.done.length, 0);
  watches = step.running;
  step = settleInstalls(watches, { p1: { command: "zsh", cwd: "/w", busy: false } });
  assert.deepEqual(step.done.map((w) => w.paneId), ["p1"]);
  assert.equal(step.running.length, 0);
});

test("closing the tab ends the install too, but a tab not listed yet doesn't", () => {
  assert.equal(settleInstalls([watch()], {}).done.length, 0);
  assert.equal(settleInstalls([watch({ seen: true })], {}).done.length, 1);
});

test("only the plugins that run the program restart", () => {
  const plugin = (id: string, exec: string[], post: string[] = []) => ({ id, permissions: { exec, post } }) as unknown as PluginView;
  const list = [plugin("pr", ["gh pr list", "az repos pr list"]), plugin("wi", ["az boards work-item show"]), plugin("hud", ["git status"]), plugin("p", [], ["az https://dev.azure.com/*"])];
  assert.deepEqual(pluginsUsing(list, "az"), ["pr", "wi", "p"]);
  assert.deepEqual(pluginsUsing(list, "glab"), []);
  assert.deepEqual(pluginsUsing([plugin("x", ["azure thing"])], "az"), [], "a program named like it isn't it");
});

test("a request waits for the dialog's answer, and fails with no dialog", async () => {
  await assert.rejects(requestToolInstall({ pluginId: "a", pluginName: "A", tool }));
  const off = onToolInstall(async (r) => r.tool.program === "az");
  assert.equal(await requestToolInstall({ pluginId: "a", pluginName: "A", tool }), true);
  off();
  await assert.rejects(requestToolInstall({ pluginId: "a", pluginName: "A", tool }));
});
