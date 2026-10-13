import assert from "node:assert/strict";
import { test } from "node:test";

import { iconMaskStyle } from "./plugin-icon-mask.ts";

test("a plugin's SVG becomes one quoted data URL that can't close its own quotes or parentheses", () => {
  const hostile = `<svg xmlns="http://www.w3.org/2000/svg" onload="window.pwned=1"><script>window.pwned=1</script><image href="https://evil.example/x"/></svg>`;
  const { maskImage } = iconMaskStyle(hostile);
  assert.match(maskImage, /^url\("data:image\/svg\+xml,[^"]*"\)$/);
  assert.ok(!maskImage.includes("<") && !maskImage.includes(" "));
  assert.equal(decodeURIComponent(maskImage.slice('url("data:image/svg+xml,'.length, -2)), hostile);
});

test("the same SVG gets the same style object, so a re-render makes no new string", () => {
  assert.equal(iconMaskStyle("<svg/>"), iconMaskStyle("<svg/>"));
});
