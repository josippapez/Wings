/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { pane, split } from "./layout.ts";
import { stillRunning } from "./still-running.ts";

test("panes a terminal host kept from a previous launch reattach by the ids saved for them, in their own project", () => {
  const saved = {
    tabs: [
      { spaceId: "web", layout: split(pane("a"), "a", "row", "b") },
      { spaceId: "api", layout: pane("c") },
      { spaceId: "removed", layout: pane("d") },
    ],
    // The host's numbers, far from the p1 this launch would start counting at.
    paneIds: { a: "p41", b: "p42", c: "p17", d: "p9" },
  };
  const live = [
    { id: "p41", spaceId: "web", cwd: "/web" },
    // Now in another project: not this pane.
    { id: "p17", spaceId: "web", cwd: "/web" },
    { id: "p9", spaceId: "removed", cwd: "/old" },
    // Its tab was closed before the quit.
    { id: "p50", spaceId: "web", cwd: "/web" },
  ];
  const running = stillRunning(saved, live, [{ id: "web" }, { id: "api" }]);
  assert.deepEqual([...running], [["a", "p41"]]);
  assert.equal(stillRunning(null, live, [{ id: "web" }]).size, 0);
});
