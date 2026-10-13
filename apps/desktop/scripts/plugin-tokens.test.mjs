// node --test: plugin-ui.css carries the values index.css says, so a theme change can't miss the plugin pages.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { apply, indexCss, pluginUiCss } from "./plugin-tokens.mjs";

test("the generated tokens in plugin-ui.css match the app's theme", () => {
  const css = readFileSync(pluginUiCss, "utf8");
  assert.equal(apply(css, readFileSync(indexCss, "utf8")), css, "run `pnpm tokens` in apps/desktop");
});
