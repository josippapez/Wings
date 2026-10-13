import assert from "node:assert/strict";
import { test } from "node:test";

import type { PluginView } from "./api.ts";
import { BLOCK_MAX_HEIGHT, BLOCK_MIN_HEIGHT_DEFAULT, BLOCK_STATE_MAX, blockStateJson, clampBlockHeight, findBlock, listBlocks } from "./plugin-blocks.ts";

const plugin = (id: string, extra: Partial<PluginView> = {}, blocks?: { id: string; title: string; icon?: string; page?: string; minHeight?: number }[]) =>
  ({
    id,
    enabled: true,
    approved: true,
    iconSvg: null,
    contributes: { ui: [], mcpTools: [], panels: [], sidebars: [], views: [], blocks: blocks?.map((b) => ({ icon: "", page: "b.html", ...b })) },
    ...extra,
  }) as unknown as PluginView;

test("listBlocks offers the blocks of plugins that are on and approved, with the default height", () => {
  const plugins = [
    plugin("a", {}, [{ id: "board", title: "Board", icon: "todo", minHeight: 240 }, { id: "list", title: "List" }]),
    plugin("off", { enabled: false }, [{ id: "x", title: "X" }]),
    plugin("new", { approved: false }, [{ id: "y", title: "Y" }]),
    plugin("none"),
  ];
  assert.deepEqual(
    listBlocks(plugins).map((b) => [b.key, b.title, b.icon, b.minHeight]),
    [["a:board", "Board", "todo", 240], ["a:list", "List", "", BLOCK_MIN_HEIGHT_DEFAULT]],
  );
});

test("findBlock fills in the default minHeight and misses a block the plugin doesn't have", () => {
  const p = plugin("a", {}, [{ id: "list", title: "List" }]);
  assert.equal(findBlock(p, "list")?.minHeight, 160);
  assert.equal(findBlock(p, "nope"), undefined);
  assert.equal(findBlock(plugin("none"), "list"), undefined);
});

test("a requested height is rounded and held between the block's minHeight and 1200", () => {
  assert.equal(clampBlockHeight(10, 200), 200);
  assert.equal(clampBlockHeight(300.6, 200), 301);
  assert.equal(clampBlockHeight(5000, 200), BLOCK_MAX_HEIGHT);
  assert.equal(BLOCK_MAX_HEIGHT, 1200);
  for (const bad of ["300", NaN, Infinity, null]) assert.throws(() => clampBlockHeight(bad, 200), /number of pixels/);
});

test("block state is limited to 16 KB of UTF-8 bytes, like Rust counts it", () => {
  assert.equal(BLOCK_STATE_MAX, 16384);
  assert.equal(blockStateJson({ a: "x".repeat(BLOCK_STATE_MAX - 8) }).length, BLOCK_STATE_MAX);
  assert.throws(() => blockStateJson({ a: "x".repeat(BLOCK_STATE_MAX) }), /Block state is limited to 16 KB/);
  // 6,000 characters but 18,000 bytes.
  assert.throws(() => blockStateJson("字".repeat(6000)), /limited to 16 KB/);
  assert.ok(blockStateJson("字".repeat(5000)));
  assert.throws(() => blockStateJson(undefined), /JSON/);
});
