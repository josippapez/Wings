import assert from "node:assert/strict";
import { test } from "node:test";

import { ingest, servers, session, startRun } from "./session.ts";

const attachment = (a) => ({ type: "attachment", attachment: a });
const tools = (extra) => attachment({ type: "deferred_tools_delta", ...extra });
const call = (id, name) => ({ type: "assistant", message: { content: [{ type: "tool_use", id, name, input: {} }] } });
const result = (id, is_error) => ({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: id, is_error }] } });
const list = (s) => servers(s).map((x) => `${x.name}:${x.state}`);

test("tool lists add and remove a server's tools, and the failed and pending lists say every server in that state", () => {
  const s = session("s");
  ingest(s, tools({ addedNames: ["mcp__github__list", "mcp__github__get", "Read"] }), true);
  assert.deepEqual(list(s), ["github:connected"]);
  ingest(s, tools({ removedNames: ["mcp__github__list", "mcp__github__get"], failedMcpServers: [{ name: "db", error: "refused" }], pendingMcpServers: ["sentry"] }), true);
  assert.deepEqual(list(s), ["db:failed", "sentry:connecting"]);
  ingest(s, tools({ failedMcpServers: [], pendingMcpServers: [] }), true);
  assert.deepEqual(list(s), []);
});

test("an entry from before this run adds no tool lists, but its model and skills still count", () => {
  const s = session("s");
  ingest(s, tools({ addedNames: ["mcp__github__list"] }), false);
  ingest(s, attachment({ type: "model", identity: { modelId: "claude-opus-5-5[1m]" } }), false);
  ingest(s, attachment({ type: "invoked_skills", skills: [{ name: "wings-plugin" }] }), false);
  assert.deepEqual(list(s), []);
  assert.equal(s.window, 1_000_000);
  assert.deepEqual([...s.skills], ["wings-plugin"]);
});

test("a result says whether its server's last call failed, and answers its call once", () => {
  const s = session("s");
  ingest(s, call("a", "mcp__github__list"), true);
  ingest(s, call("b", "Read"), true);
  ingest(s, result("a", true), true);
  assert.equal(s.lastFailed.get("github"), true);
  ingest(s, call("c", "mcp__github__get"), true);
  ingest(s, result("c", false), true);
  assert.equal(s.lastFailed.get("github"), false);
  assert.equal(s.calls.size, 0);
  assert.deepEqual([...s.used], ["github"]);
});

test("the context is the newest reply's input, cached or not, and a new run starts over", () => {
  const s = session("s");
  ingest(s, { type: "assistant", message: { usage: { input_tokens: 10, cache_creation_input_tokens: 20, cache_read_input_tokens: 300 } } }, true);
  assert.equal(s.tokens, 330);
  startRun(s, 5);
  assert.equal(s.tokens, null);
  assert.equal(s.startedAt, 5);
});
