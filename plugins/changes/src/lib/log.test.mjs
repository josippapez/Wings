import assert from "node:assert/strict";
import { test } from "node:test";

import { describeRef, parseLog } from "./log.ts";

const entry = (hash, parents, refs, subject) => [hash, parents, "Ada", "100", "90", refs, subject].join("\0") + "\0";

test("parseLog reads git log -z with full decorations, and leaves origin/HEAD out", () => {
  const out = entry("h1", "p1 p2", "HEAD -> refs/heads/main, tag: refs/tags/v1, refs/remotes/origin/main, refs/remotes/origin/HEAD", "Merge it") + entry("p1", "", "", "Root");
  const [merge, root] = parseLog(out);
  assert.deepEqual(merge.refs, ["H:main", "t:v1", "r:origin/main"]);
  assert.deepEqual([merge.parents, merge.at, merge.ct, merge.author, merge.subject], [["p1", "p2"], 100, 90, "Ada", "Merge it"]);
  assert.deepEqual(root.parents, []);
});

test("describeRef says what a ref is, the same for the History list and the graph", () => {
  assert.deepEqual(describeRef("H:main"), { head: true, label: "main", title: "HEAD, on main", tone: "info" });
  assert.deepEqual(describeRef("t:v1"), { head: false, label: "◆ v1", title: "Tag v1", tone: "warn" });
  assert.deepEqual(describeRef("r:origin/main"), { head: false, label: "origin/main", title: "Remote branch origin/main", tone: undefined });
});
