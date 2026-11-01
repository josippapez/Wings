/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { createHighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import githubDark from "shiki/themes/github-dark.mjs";
import tsx from "shiki/langs/tsx.mjs";

import { tokensOf } from "./highlight-tokens.ts";

test("a slow first tokenization, like a cold grammar in WebKit, still colors every line", async () => {
  const shiki = await createHighlighterCore({ themes: [githubDark], langs: [tsx], engine: createJavaScriptRegexEngine({ forgiving: true }) });
  const code = "const a = 1;\n<script\n  nonce={nonce}\n/>";
  const warm = tokensOf(shiki, code, "tsx");
  // Every look at the clock finds 600 ms gone, as when the engine is compiling the grammar's patterns.
  const realNow = Date.now;
  let clock = 0;
  Date.now = () => (clock += 600);
  try {
    assert.deepEqual(tokensOf(shiki, code, "tsx"), warm);
  } finally {
    Date.now = realNow;
  }
  assert.ok(warm.every((line) => line.some(([, color]) => color)), "every line has a colored token");
});
