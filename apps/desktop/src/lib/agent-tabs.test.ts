import assert from "node:assert/strict";
import { test } from "node:test";

import { idleAgentTab, type AgentTab } from "./agent-tabs.ts";
import type { Agent, PaneInfo } from "./api.ts";

const tabs = new Map<string, AgentTab>([
  ["p1", { spaceId: "s1", cwd: null, owner: null }],
  ["p2", { spaceId: "s1", cwd: "/r/app", owner: null }],
  ["p3", { spaceId: "s1", cwd: null, owner: null }],
  ["p4", { spaceId: "s1", cwd: null, owner: "a1" }],
]);
const shell = (busy: boolean): PaneInfo => ({ command: busy ? "npm" : "zsh", cwd: "/r", busy });

test("a command goes to an agent's tab in the same folder that sits at its prompt", () => {
  const open = new Set(["p1", "p2", "p3"]);
  const info = { p1: shell(true), p2: shell(false), p3: shell(false) };
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: null, owner: null }, open, info, []), "p3", "p1 still runs its command");
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: "/r/app", owner: null }, open, info, []), "p2");
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: "/r/other", owner: null }, open, info, []), "p2", "another folder of the project reuses an idle pane, which cds there");
  assert.equal(idleAgentTab(tabs, { spaceId: "s2", cwd: null, owner: null }, open, info, []), undefined, "another project");
});

test("an agent's split below it is reused by that agent only", () => {
  const open = new Set(["p3", "p4"]);
  const info = { p3: shell(false), p4: shell(false) };
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: null, owner: "a1" }, open, info, []), "p4");
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: null, owner: "a2" }, open, info, []), undefined);
});

test("a closed tab, one with an agent in it, or one Wings knows nothing about yet isn't reused", () => {
  const info = { p1: shell(false), p3: shell(false) };
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: null, owner: null }, new Set(["p3"]), info, [{ paneId: "p3" } as Agent]), undefined);
  assert.equal(idleAgentTab(tabs, { spaceId: "s1", cwd: null, owner: null }, new Set(["p1"]), {}, []), undefined);
});
