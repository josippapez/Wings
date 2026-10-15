import assert from "node:assert/strict";
import { test } from "node:test";

import { fakeWings, ok, repos } from "../lib/fake-wings.mjs";
import { serveSidebar } from "./sidebar.ts";

const fake = fakeWings();
const pane = (paneId, cwd, focused = false) => ({ paneId, cwd, focused });
const changes = async () => (await fake.ask("changes", { mode: "uncommitted" })).repo;

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

test("a Changes answer too big for one message lists the files that fit, whatever their path lengths, and says how many there are", async () => {
  serveSidebar(() => [pane("a", "/w/a5", true)]);
  // The files that fit end at a different place for each length, and some end close to the limit, where the envelope the answer travels in counts.
  for (let length = 20; length < 60; length++) {
    const paths = Array.from({ length: 3000 }, (_, i) => `src/${String(i).padStart(length, "0")}.ts`);
    fake.reply = (args, cwd) =>
      args.startsWith("diff") && args.includes("--name-status")
        ? ok(paths.map((p) => `M\0${p}`).join("\0") + "\0")
        : args.startsWith("diff") && args.includes("--numstat")
          ? ok(paths.map((p) => `1\t0\t${p}`).join("\0") + "\0")
          : repos()(args, cwd);
    const answer = await fake.ask("changes", { mode: "uncommitted" });
    assert.equal(answer.total, 3000);
    assert.ok(answer.files.length > 500 && answer.files.length < 3000, `${answer.files.length} files listed`);
  }
});

test("the commit ask only takes a full hash, since it goes to git show as an argument", async () => {
  serveSidebar(() => []);
  await assert.rejects(fake.ask("commit", { root: "/w/a6", hash: "--output=/tmp/x", subject: "", author: "", at: 0 }), /isn't a commit hash/);
});
