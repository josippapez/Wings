import test from "node:test";
import assert from "node:assert/strict";
import { issueKeyIn, localOnlyDenial } from "./local-only.ts";

const WEB = ["WEB", "API"];

test("a key in a commit or pull request command is denied, whichever way the message is given", () => {
  assert.match(localOnlyDenial('git commit -m "WEB-12 fix totals"', WEB), /^WEB-12 is a local Wings issue/);
  assert.match(localOnlyDenial("git add . && git commit -m \"$(cat <<'EOF'\nFix totals\n\nRefs API-3\nEOF\n)\"", WEB), /^API-3 /);
  assert.match(localOnlyDenial('git -C /work/web commit -m "fix (WEB-1)"', WEB), /WEB-1/);
  assert.match(localOnlyDenial('gh pr create --title "WEB-9 totals" --body x', WEB), /WEB-9/);
  assert.match(localOnlyDenial('glab mr note 4 -m "see WEB-2"', WEB), /WEB-2/);
});

test("text that only looks like a key, a prefix the project doesn't use and commands that don't write are allowed", () => {
  assert.equal(localOnlyDenial('git commit -m "Switch to UTF-8"', WEB), null);
  assert.equal(localOnlyDenial('git commit -m "ABC-12 fix"', WEB), null);
  assert.equal(localOnlyDenial('git commit -m "XWEB-12 and web-12"', WEB), null);
  assert.equal(localOnlyDenial("git log --grep WEB-12", WEB), null);
  assert.equal(localOnlyDenial("rg WEB-12 src", WEB), null);
  assert.equal(issueKeyIn("WEB-12", []), null);
});
