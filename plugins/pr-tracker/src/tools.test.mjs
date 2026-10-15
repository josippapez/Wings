import assert from "node:assert/strict";
import test from "node:test";
import { registerTools } from "wings-plugin-sdk/tools";
import { tools } from "./tools.ts";

test("failing_checks takes no input: a field is refused with its name before the tracker looks at a pane", async () => {
  const handlers = new Map();
  registerTools(tools, { onTool: (name, handler) => handlers.set(name, handler) });
  assert.deepEqual([...handlers.keys()], ["failing_checks"]);
  const call = (input, paneId = null) => handlers.get("failing_checks")(input, { paneId });
  await assert.rejects(async () => call({ pane: "p1" }), { message: "Unknown field: pane. failing_checks takes no input." });
  assert.match(await call({}), /found no pull request for this pane's branch/);
});
