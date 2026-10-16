import assert from "node:assert/strict";
import { test } from "node:test";

import { fakeWings, repos } from "../lib/fake-wings.mjs";
import { ch } from "../lib/messages.ts";
import { serveSidebar } from "./sidebar.ts";

const fake = fakeWings();
const pane = (paneId, cwd, focused = false) => ({ paneId, cwd, focused });
const changes = async () => (await ch.get("changes", { mode: "uncommitted" })).repo;

// Each test uses folders of its own: the SDK keeps the repo of a folder it found.

test("the sidebar follows the focused pane's repo, and keeps it while the focus is on a pane outside one", async () => {
  fake.reply = repos("/tmp/plain1");
  const panes = [pane("a", "/w/a1", true), pane("b", "/w/b1"), pane("c", "/tmp/plain1")];
  serveSidebar(() => panes);
  assert.equal((await changes()).root, "/w/a1");
  [panes[0].focused, panes[2].focused] = [false, true];
  assert.equal((await changes()).root, "/w/a1", "kept while another pane is still in it");
  panes.shift();
  assert.equal((await changes()).root, "/w/b1", "its panes are gone: the first pane in a repo");
});

test("a focused pane outside a repo shows the first pane's repo when the sidebar showed none yet", async () => {
  fake.reply = repos("/tmp/plain2", "/tmp/plain2b");
  serveSidebar(() => [pane("a", "/tmp/plain2", true), pane("b", "/tmp/plain2b"), pane("c", "/w/b2")]);
  assert.equal((await changes()).root, "/w/b2");
});

test("no pane in a repo is an answer of its own", async () => {
  fake.reply = repos("/tmp/plain3");
  serveSidebar(() => [pane("a", "/tmp/plain3", true)]);
  assert.equal(await changes(), null);
});

test("a repo lookup that failed is asked again next time, not kept", async () => {
  let refused = true;
  fake.reply = (args, cwd) => (refused && args === "rev-parse --show-toplevel" ? new Error("changes may not run git rev-parse") : repos()(args, cwd));
  serveSidebar(() => [pane("a", "/w/a4", true)]);
  assert.equal(await changes(), null);
  refused = false;
  assert.equal((await changes()).root, "/w/a4");
});

test("the commit ask only takes a full hash, since it goes to git show as an argument", async () => {
  serveSidebar(() => []);
  await assert.rejects(fake.ask("commit", { root: "/w/a6", hash: "--output=/tmp/x", subject: "", author: "", at: 0 }), /isn't a commit hash/);
});

test("main's poll reloads a shown history only when Wings doesn't watch its repo, since a git change reloads the rest", async () => {
  const sidebar = serveSidebar(() => [pane("a", "/w/a7", true)]);
  const loads = () => fake.sent.filter((m) => (m.kind === "state" || m.kind === "fresh") && m.name === "history").length;
  const waitFor = async (n) => {
    while (loads() < n) await new Promise((resolve) => setTimeout(resolve, 1));
  };
  fake.projects = [];
  fake.heard({ "wings:sdk": 1, kind: "sub", from: "page", subs: [["history", { query: "", all: false }]] });
  await waitFor(1);
  sidebar.poll();
  await waitFor(2);
  fake.projects = [{ id: "p", path: "/w/a7" }];
  sidebar.poll();
  await waitFor(3);
  const before = loads();
  sidebar.poll();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(loads(), before);
  fake.projects = [];
});
