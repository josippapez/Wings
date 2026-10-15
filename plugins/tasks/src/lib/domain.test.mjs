import test from "node:test";
import assert from "node:assert/strict";
import { DESCRIPTION_TEMPLATE, hasContent, prefixFor, visibleDescription } from "./domain.ts";

test("key prefix: letters only, up to three, uppercase, ISS when none", () => {
  assert.equal(prefixFor("web"), "WEB");
  assert.equal(prefixFor("my-app-2"), "MYA");
  assert.equal(prefixFor("42-x"), "X");
  assert.equal(prefixFor("12345"), "ISS");
  assert.equal(prefixFor("Ünï"), "N");
  assert.equal(prefixFor(""), "ISS");
});

test("description template: helper, and sections with nothing in them don't count as content", () => {
  assert.equal(DESCRIPTION_TEMPLATE, "## Goal\n\n## Scope\n\n## Done when\n- [ ] \n\n## Validation\n");
  assert.equal(hasContent(DESCRIPTION_TEMPLATE), false);
  assert.equal(hasContent(undefined), false);
  assert.equal(hasContent("  \n"), false);
  assert.equal(hasContent("## Goal\nShip it\n\n## Scope\n"), true);
  assert.equal(hasContent("## Done when\n- [x] done\n"), true);
  assert.equal(hasContent("plain text, no headings"), true);
  assert.equal(visibleDescription("## Goal\nShip it\n\n## Scope\n\n## Done when\n- [ ] \n- [ ] tests\n\n## Validation\n"), "## Goal\nShip it\n\n## Done when\n- [ ] \n- [ ] tests");
  assert.equal(visibleDescription("Intro\n\n## Goal\n"), "Intro");
  assert.equal(visibleDescription(DESCRIPTION_TEMPLATE), "");
});
