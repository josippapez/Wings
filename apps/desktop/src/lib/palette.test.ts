/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { groupOf, matchScore, rank, type PaletteItem } from "./palette.ts";

const item = (kind: PaletteItem["kind"], title: string, more: Partial<PaletteItem> = {}): PaletteItem => ({ id: `${kind}:${title}`, kind, title, run: () => {}, ...more });
const titles = (items: PaletteItem[]) => items.map((i) => i.title);

test("every word has to match, in any order", () => {
  assert.notEqual(matchScore("split right", item("command", "Split Right")), null);
  assert.notEqual(matchScore("right split", item("command", "Split Right")), null);
  assert.equal(matchScore("split left", item("command", "Split Right")), null);
});

test("letters in order match, letters out of order don't", () => {
  assert.notEqual(matchScore("nwtb", item("command", "New Tab")), null);
  assert.equal(matchScore("btw", item("command", "New Tab")), null);
});

test("the start of the title beats the start of a later word beats the middle", () => {
  const score = (title: string) => matchScore("tab", item("command", title))!;
  assert.ok(score("Tab Switcher") > score("New Tab"));
  assert.ok(score("New Tab") > score("Stable"));
});

test("a run of the word beats the same letters spread out", () => {
  assert.ok(matchScore("side", item("command", "Toggle Sidebar"))! > matchScore("side", item("command", "Split Down Even"))!);
});

test("the subtitle and keywords are searched, at less weight than the title", () => {
  const byTitle = item("project", "wings");
  const bySubtitle = item("project", "other", { subtitle: "wings" });
  assert.ok(matchScore("wings", byTitle)! > matchScore("wings", bySubtitle)!);
  assert.notEqual(matchScore("preferences", item("command", "Settings", { keywords: "preferences" })), null);
});

test("nothing typed: agents that need you come first, then each kind in turn", () => {
  const items = [
    item("command", "New Tab"),
    item("project", "api"),
    item("agent", "worker", { state: "working" }),
    item("agent", "finished", { state: "done" }),
    item("tab", "main"),
    item("agent", "stuck", { state: "blocked" }),
    item("session", "yesterday"),
  ];
  const out = rank(items, "");
  assert.deepEqual(titles(out), ["stuck", "finished", "worker", "New Tab", "api", "main", "yesterday"]);
  assert.equal(groupOf(out[0]), "Needs you");
  assert.equal(groupOf(out[2]), "Agents");
  assert.equal(groupOf(out[3]), "Commands");
});

test("typed: best match first, and an agent that needs you ahead of an equal one that doesn't", () => {
  const items = [item("agent", "api server", { state: "idle" }), item("agent", "api worker", { state: "blocked" }), item("project", "api")];
  assert.deepEqual(titles(rank(items, "api")).slice(0, 1), ["api"]);
  const agents = rank(items.filter((i) => i.kind === "agent"), "api");
  assert.deepEqual(titles(agents), ["api worker", "api server"]);
});

test("typed: items that don't match are left out, and a tie keeps the order given", () => {
  const items = [item("command", "Zoom Pane"), item("command", "Find in Pane"), item("project", "docs")];
  assert.deepEqual(titles(rank(items, "pane")).sort(), ["Find in Pane", "Zoom Pane"]);
  assert.deepEqual(titles(rank([item("command", "Alpha One"), item("command", "Alpha Two")], "alpha")), ["Alpha One", "Alpha Two"]);
});

test("the list is capped", () => {
  const many = Array.from({ length: 300 }, (_, i) => item("session", `session ${i}`));
  assert.equal(rank(many, "", 80).length, 80);
  assert.equal(rank(many, "session", 80).length, 80);
});

test("ranking 400 items per keystroke stays well under a frame", () => {
  const many = Array.from({ length: 400 }, (_, i) => item(i % 2 ? "session" : "command", `Some longer item title number ${i} for the palette`, { subtitle: `/Users/someone/project-${i}/src`, keywords: "alpha beta gamma" }));
  const start = performance.now();
  for (const q of ["s", "so", "som", "some l", "xyz", "title 39"]) rank(many, q);
  const perKeystroke = (performance.now() - start) / 6;
  console.log(`rank(400 items): ${perKeystroke.toFixed(2)} ms per keystroke`);
  assert.ok(perKeystroke < 8, `${perKeystroke} ms`);
});
