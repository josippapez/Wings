import assert from "node:assert/strict";
import { test } from "node:test";

import { relay, settle } from "./fake-frames.mjs";
import { setting } from "./settings.js";

const view = (wings) => setting("view", { default: "board", check: (v) => (v === "rows" || v === "list" ? v : undefined), wings });

test("a setting is its default until something it accepts is stored", async () => {
  const { wings } = relay()();
  const s = view(wings);
  assert.equal(await s.get(), "board");
  await wings.storage.set("view", "list");
  assert.equal(await s.get(), "list");
  await wings.storage.set("view", "sideways");
  assert.equal(await s.get(), "board");
  assert.equal(await setting("flag", { default: false, wings }).get(), false);
});

test("set is stored, and onChange hears it in the frame that set it and in every other", async () => {
  const frame = relay();
  const [page, main] = [frame(), frame()];
  const [here, there] = [view(page.wings), view(main.wings)];
  const heard = [];
  const stop = there.onChange((v) => heard.push(["main", v]));
  here.onChange((v) => heard.push(["page", v]));
  await here.set("rows");
  await settle();
  assert.deepEqual(heard.sort(), [["main", "rows"], ["page", "rows"]]);
  assert.equal(await there.get(), "rows");
  stop();
  await here.set("list");
  await settle();
  assert.deepEqual(heard.sort(), [["main", "rows"], ["page", "list"], ["page", "rows"]]);
});
