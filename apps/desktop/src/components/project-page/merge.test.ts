/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { findFact, freshTitle, imageExt, imageMarkdown, takeStored, type EditorState } from "./merge.ts";

const state = (over: Partial<EditorState> = {}): EditorState => ({ base: "abc", text: "abc", dirty: false, inflight: false, sent: false, stale: "", ...over });

test("an untouched editor takes whatever storage holds", () => {
  assert.deepEqual(takeStored(state(), "abc\n\nfrom an agent"), { text: "abc\n\nfrom an agent", base: "abc\n\nfrom an agent" });
  assert.deepEqual(takeStored(state(), "rewritten"), { text: "rewritten", base: "rewritten" });
});

test("an agent's append goes after what you typed, which stays", () => {
  const s = state({ text: "abc and mine", dirty: true });
  assert.deepEqual(takeStored(s, "abc\n\nappended"), { text: "abc and mine\n\nappended", base: "abc\n\nappended" });
});

test("a rewrite while you have typed is not taken", () => {
  assert.equal(takeStored(state({ text: "abc mine", dirty: true }), "something else"), null);
});

test("nothing is taken while a save is out, or when storage still shows the base", () => {
  assert.equal(takeStored(state({ inflight: true }), "abc more"), null);
  assert.equal(takeStored(state(), "abc"), null);
});

test("after a save, storage catching up with it is ignored but an extension is taken", () => {
  const s = state({ base: "abc mine", text: "abc mine", sent: true, stale: "abc" });
  assert.equal(takeStored(s, "abc"), null);
  assert.deepEqual(takeStored(s, "abc mine\n\nmore"), { text: "abc mine\n\nmore", base: "abc mine\n\nmore" });
  assert.equal(takeStored(s, "other"), null);
});

test("a fact is found by key ignoring case and spaces", () => {
  const facts = [{ key: "Dev" }, { key: "Test" }];
  assert.equal(findFact(facts, " dev "), facts[0]);
  assert.equal(findFact(facts, "Build"), undefined);
});

test("a new section's title is one nobody has, ignoring case", () => {
  assert.equal(freshTitle([]), "Untitled");
  assert.equal(freshTitle(["untitled", "Untitled 2"]), "Untitled 3");
});

test("only raster images are stored, and the Markdown names the stored file", () => {
  assert.equal(imageExt("image/png"), "png");
  assert.equal(imageExt("image/jpeg"), "jpg");
  assert.equal(imageExt("image/svg+xml"), null);
  assert.equal(imageMarkdown("a1.png"), "![](wings-image:a1.png)");
  assert.equal(imageMarkdown("wings-image:a1.png"), "![](wings-image:a1.png)");
});
