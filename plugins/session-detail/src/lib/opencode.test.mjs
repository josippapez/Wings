import assert from "node:assert/strict";
import { test } from "node:test";

import { applyActivity, followOpencode } from "./opencode.ts";
import { session } from "./session.ts";

const tool = (id, name, input, extra) => ({ kind: "tool", id, at: 1000, name, input, status: "completed", ms: 40, result: "ok", files: [], ...extra });
const activity = (items, extra) => ({ model: "gpt-x", context: 900, tokens: { input: 10, output: 5, reasoning: 2, cacheRead: 300, cacheWrite: 1 }, cost: 0.5, items, ...extra });

test("an activity becomes rows, running calls, edited files and the session's numbers", () => {
  const s = session("ses_a", "opencode");
  applyActivity(
    s,
    activity([
      { kind: "prompt", id: "m1", at: 900, text: "fix the bug" },
      tool("c1", "patch", { patchText: "…" }, { files: ["/w/src/a.ts", "/w/src/b.ts"] }),
      tool("c2", "shell", { command: "git commit -m 'Fix it'" }),
      tool("c3", "read", { path: "src/c.ts" }, { status: "error", result: "File not found" }),
      tool("c4", "grep", { pattern: "TODO" }, { status: "running", ms: null, result: null }),
    ]),
  );
  assert.deepEqual(s.items.map((i) => [i.kind, i.key, i.summary]), [["prompt", "m1", "fix the bug"], ["tool", "c1", "a.ts, b.ts"], ["tool", "c2", "Fix it"], ["tool", "c3", "c.ts"], ["tool", "c4", "TODO"]]);
  assert.deepEqual(s.items[2].tags, [{ kind: "commit", label: "Commit" }]);
  assert.deepEqual([s.items[3].error, s.items[3].done, s.items[4].done], [true, true, false]);
  assert.deepEqual([...s.tools.keys()], ["c4"]);
  assert.deepEqual([...s.files], [["/w/src/a.ts", 1], ["/w/src/b.ts", 1]]);
  assert.deepEqual([s.model, s.context, s.cost, s.tokens.output], ["gpt-x", 900, 0.5, 7]);
});

test("reading again replaces what was there", () => {
  const s = session("ses_a", "opencode");
  applyActivity(s, activity([tool("c1", "read", { path: "a" }, { status: "running", ms: null })]));
  applyActivity(s, activity([tool("c1", "read", { path: "a" }), tool("c2", "read", { path: "b" })]));
  assert.deepEqual([s.items.length, s.tools.size], [2, 0]);
});

test("a pane with an OpenCode session loads once, and again when it works or Wings says it did something", async () => {
  let onPanes, onActivity;
  const reads = [];
  const w = {
    onPanes: (fn) => (onPanes = fn),
    onAgentActivity: (fn) => (onActivity = fn),
    agentActivity: async (id) => (reads.push(id), activity([])),
  };
  const changed = [];
  const follow = followOpencode((s) => changed.push(s.state), w);
  const pane = (state, kind = "opencode") => [{ paneId: "p", cwd: "/w", project: "web", session: { sessionId: "ses_a", kind, name: "n", state, startedAt: 1 } }];
  const settle = () => new Promise((r) => setTimeout(r, 0));

  onPanes(pane("idle", "claude"));
  assert.deepEqual([follow.sessions().length, reads.length], [0, 0], "Claude's sessions follow their transcript");
  onPanes(pane("idle"));
  onPanes(pane("idle"));
  await settle();
  assert.equal(reads.length, 1);
  onPanes(pane("working"));
  await settle();
  assert.equal(reads.length, 2, "a run starting is read");
  onActivity({ sessionId: "ses_other" });
  onActivity({ sessionId: "ses_a" });
  await settle();
  assert.equal(reads.length, 3);
  assert.deepEqual([follow.get("ses_a").project, follow.get("ses_a").state], ["web", "working"]);
  onPanes([]);
  assert.equal(follow.sessions().length, 0);
});
