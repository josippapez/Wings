import assert from "node:assert/strict";
import { test } from "node:test";

import { pluginIconNames, pluginIcons } from "./plugin-icons.ts";

test("every icon a manifest may name has a drawing, and every drawing is allowed", () => {
  assert.deepEqual([...pluginIconNames].sort(), Object.keys(pluginIcons).sort());
});
