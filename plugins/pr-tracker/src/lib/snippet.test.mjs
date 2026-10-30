import assert from "node:assert/strict";
import test from "node:test";
import { githubComment } from "./forges/github.ts";
import { glComments } from "./forges/gitlab.ts";
import { diffWindow } from "./snippet.ts";

const hunk = ["@@ -10,6 +20,7 @@ fn", " a", " b", "-old1", "-old2", "+new1", "+new2", " c", " d", "+tail"].join("\n");
const view = (lines) => lines.map((l) => `${l.n}${l.added ? "+" : l.removed ? "-" : " "}${l.hit ? "*" : " "}${l.text}`);

test("a diff window ends at the commented line and marks +/- rows, numbering removed rows by their old line", () => {
  assert.deepEqual(view(diffWindow(hunk, "additions", 25)), ["13- old2", "22+ new1", "23+ new2", "24  c", "25 *d"]);
  assert.deepEqual(view(diffWindow(hunk, "additions", 22, 23)), ["20  a", "21  b", "12- old1", "13- old2", "22+*new1", "23+*new2"]);
  assert.deepEqual(view(diffWindow(hunk, "deletions", 13)), ["20  a", "21  b", "12- old1", "13-*old2"]);
});

test("a diff window is null when the diff has no such line, and a long range shows its first rows", () => {
  assert.equal(diffWindow(hunk, "additions", 99), null);
  assert.equal(diffWindow("not a diff", "additions", 1), null);
  const long = `@@ -1,0 +1,12 @@\n${Array.from({ length: 12 }, (_, i) => `+l${i + 1}`).join("\n")}`;
  assert.deepEqual(diffWindow(long, "additions", 2, 12).filter((l) => l.hit).map((l) => l.n), [2, 3, 4, 5, 6, 7]);
});

test("a GitHub thread carries its code from the comment's diff hunk, and an outdated one from the original lines", () => {
  const c = { id: 1, path: "a.ts", line: 25, side: "RIGHT", created_at: "", html_url: "", diff_hunk: hunk };
  assert.equal(githubComment(c).snippet.at(-1).n, 25);
  assert.deepEqual(githubComment({ ...c, line: 23, start_line: 22, start_side: "RIGHT" }).snippet.filter((l) => l.hit).map((l) => l.n), [22, 23]);
  assert.deepEqual(githubComment({ ...c, line: null, original_line: 23, original_start_line: 22 }).snippet.filter((l) => l.hit).map((l) => l.n), [22, 23]);
  assert.equal(githubComment({ ...c, line: 40, original_line: 25 }).snippet.at(-1).n, 25, "the hunk's numbers are the original ones, not where the line sits now");
  assert.equal(githubComment({ ...c, in_reply_to_id: 9 }).snippet, undefined, "a reply shows no code");
  assert.equal(githubComment({ ...c, diff_hunk: undefined }).snippet, undefined);
});

test("a GitLab diff note says its commits and lines, so its snippet can be read from the diff between them", () => {
  const note = { id: 5, type: "DiffNote", resolvable: true, created_at: "", author: { username: "ada" }, position: { new_path: "a.ts", old_path: "a.ts", new_line: 7, old_line: null, base_sha: "b1", head_sha: "h1", line_range: { start: { new_line: 5, old_line: null } } } };
  const [root, reply] = glComments([{ id: "d", notes: [note, { ...note, id: 6 }] }], "u");
  assert.deepEqual([root.range, root.refs, reply.range, reply.refs], [{ start: 5, end: 7 }, { base: "b1", head: "h1" }, undefined, undefined]);
});
