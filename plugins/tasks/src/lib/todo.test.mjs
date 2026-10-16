import test from "node:test";
import assert from "node:assert/strict";
import { currentStep, itemsOf, railOf, withItems } from "./todo.ts";

const who = { sessionId: "s1", sessionName: "agent", projectId: "p1" };
const step = (text, status = "pending") => ({ text, status });

test("each write adds a version, an identical list adds none, and only the newest 50 are kept", () => {
  let list = withItems(null, who, [step("a")], undefined, 1);
  assert.equal(withItems(list, who, [step("a")], undefined, 2), list);
  list = withItems(list, who, [step("a", "done"), step("b")], "WEB-4", 3);
  assert.deepEqual(list.versions.map((v) => v.at), [1, 3]);
  assert.equal(list.issueKey, "WEB-4");
  assert.equal(itemsOf(list).length, 2);
  for (let i = 0; i < 60; i++) list = withItems(list, who, [step(`s${i}`)], undefined, 10 + i);
  assert.equal(list.versions.length, 50);
  assert.equal(itemsOf(list)[0].text, "s59");
});

test("the card shows the step in progress while any step is open, and nothing once all are done", () => {
  const items = [step("a", "done"), step("b"), step("c", "in_progress")];
  assert.equal(currentStep(items).text, "c");
  const list = withItems(null, who, items, "WEB-4", 1);
  assert.equal(railOf(list, "todo").title, "Todo · WEB-4");
  assert.equal(railOf(withItems(list, who, [step("a", "done")], undefined, 2), "todo"), null);
});
