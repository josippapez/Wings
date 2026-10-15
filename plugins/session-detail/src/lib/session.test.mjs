import assert from "node:assert/strict";
import { test } from "node:test";

import { checklist, fresh, ingest, session } from "./session.ts";

const T = (s) => new Date(1_000_000 + s * 1000).toISOString();
const reply = (s, content, extra) => ({ type: "assistant", timestamp: T(s), message: { id: `m${s}`, model: "claude-sonnet-5-5", content, ...extra } });
const use = (id, name, input) => ({ type: "tool_use", id, name, input });
const results = (s, ...blocks) => ({ type: "user", timestamp: T(s), message: { content: blocks.map(([id, content, is_error]) => ({ type: "tool_result", tool_use_id: id, content, is_error })) } });

test("a result finishes its call and takes its time, error and output", () => {
  const s = session("s");
  ingest(s, reply(0, [use("a", "Edit", { file_path: "/w/src/a.ts" }), use("b", "Bash", { command: "pnpm test" })]), true);
  assert.deepEqual([...s.files], [["/w/src/a.ts", 1]]);
  assert.equal(s.items[0].summary, "a.ts");
  ingest(s, results(2, ["a", "ok"], ["b", [{ type: "text", text: "boom" }], true]), true);
  assert.deepEqual(s.items.map((i) => [i.done, i.error, i.ms, i.result]), [[true, false, 2000, "ok"], [true, true, 2000, "boom"]]);
  assert.equal(s.tools.size, 0);
});

test("tokens add up over a run's replies, and an entry from before the run only sets the model and context", () => {
  const s = session("s");
  const usage = { input_tokens: 5, output_tokens: 7, cache_read_input_tokens: 100, cache_creation_input_tokens: 20 };
  ingest(s, reply(0, [use("a", "Read", { file_path: "/w/a" })], { usage }), false);
  assert.deepEqual([s.model, s.context, s.items.length, s.tokens.output], ["claude-sonnet-5-5", 125, 0, 0]);
  ingest(s, reply(1, [], { usage }), true);
  ingest(s, reply(2, [], { usage }), true);
  assert.deepEqual(s.tokens, { input: 10, output: 14, cacheRead: 200, cacheWrite: 40 });
});

test("a message you send while Claude works is a prompt, and Claude's own wrappers aren't", () => {
  const s = session("s");
  const queued = (prompt, humanTurn) => ({ type: "attachment", timestamp: T(0), attachment: { type: "queued_command", humanTurn, prompt } });
  ingest(s, queued("also add tests", true), true);
  ingest(s, queued("<task-notification>done</task-notification>", true), true);
  ingest(s, queued("from a hook", false), true);
  ingest(s, { type: "user", timestamp: T(1), message: { content: "[Request interrupted by user]" } }, true);
  assert.deepEqual(s.items.map((i) => [i.kind, i.text]), [["prompt", "also add tests"]]);
});

test("a task is numbered by the result of its create, and the task list wins over TodoWrite", () => {
  const s = session("s");
  ingest(s, reply(0, [use("w", "TodoWrite", { todos: [{ content: "one", status: "pending" }] })]), true);
  assert.deepEqual(checklist(s).map((t) => t.text), ["one"]);
  ingest(s, reply(1, [use("c", "TaskCreate", { subject: "build it" })]), true);
  ingest(s, results(2, ["c", "Task #7 created successfully"]), true);
  ingest(s, reply(3, [use("u", "TaskUpdate", { taskId: 7, status: "completed" })]), true);
  assert.deepEqual(checklist(s), [{ text: "build it", active: null, status: "completed" }]);
});

test("a run keeps its newest 150 rows, and a new run starts with none", () => {
  const s = session("s");
  for (let i = 0; i < 200; i++) ingest(s, reply(i, [use(`t${i}`, "Read", { file_path: `/w/${i}` })]), true);
  assert.deepEqual([s.items.length, s.items[0].key], [150, "t50"]);
  Object.assign(s, fresh());
  assert.equal(s.items.length, 0);
});
