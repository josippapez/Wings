import assert from "node:assert/strict";
import { test } from "node:test";

import { openChanges } from "./diff.ts";
import { fakeWings, ok, repos } from "./fake-wings.mjs";

const fake = fakeWings();
const updates = [];
/** The viewer's Refresh button. */
let refresh;
Object.assign(wings, {
  focused: false,
  onFocus: () => () => {},
  onDiffRefresh: (handler) => void (refresh = handler),
  onDiffClosed: () => () => {},
  openDiff: async () => ({ id: "changes:1" }),
  updateDiff: async (id, update) => void updates.push(update),
});

test("the open viewer's Refresh reads git again, so it shows a new edit and that everything got committed", async () => {
  /** The one changed file's new line, or null when everything is committed. */
  let line = "one";
  fake.reply = (args, cwd) => {
    if (!args.startsWith("diff ")) return repos()(args, cwd);
    if (line === null) return ok();
    if (args.includes("--numstat")) return ok("1\t0\ta.txt\0");
    if (args.includes("--name-status")) return ok("M\0a.txt\0");
    return ok(`diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -0,0 +1 @@\n+${line}\n`);
  };

  await openChanges("/w/live1", "uncommitted", null);
  assert.match(updates.at(-1).patch, /\+one/);
  line = "two";
  await refresh({ diffId: "changes:1" });
  assert.match(updates.at(-1).patch, /\+two/);
  line = null;
  await refresh({ diffId: "changes:1" });
  assert.deepEqual(updates.at(-1), { error: "No uncommitted changes." });
});
