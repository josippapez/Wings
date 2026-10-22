import assert from "node:assert/strict";
import { test } from "node:test";

import { contains, describe, goneBranches, hasWorktrees, parseWorktrees, shortenPath } from "./worktrees.ts";

const PORCELAIN = `worktree /Users/me/dev/app
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /Users/me/dev/app-feature
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature/x

worktree /Users/me/dev/app-detached
HEAD 3333333333333333333333333333333333333333
detached

worktree /Users/me/dev/app-gone
HEAD 4444444444444444444444444444444444444444
branch refs/heads/old
locked testing the lock
prunable gitdir file points to non-existent location

worktree /Users/me/dev/has space
HEAD 5555555555555555555555555555555555555555
branch refs/heads/spaced
locked
`;

test("parses git worktree list --porcelain, first entry is the main worktree", () => {
  const list = parseWorktrees(PORCELAIN);
  assert.equal(list.length, 5);
  assert.deepEqual(list.map((w) => w.main), [true, false, false, false, false]);
  assert.equal(list[1].branch, "feature/x");
  assert.equal(list[2].detached, true);
  assert.equal(list[2].branch, null);
  assert.equal(list[3].locked, "testing the lock");
  assert.equal(list[3].prunable, "gitdir file points to non-existent location");
  assert.equal(list[4].path, "/Users/me/dev/has space");
  assert.equal(list[4].locked, "");
  assert.deepEqual(parseWorktrees(""), []);
});

test("a bare main worktree is read", () => {
  const [bare] = parseWorktrees("worktree /srv/repo.git\nbare\n");
  assert.equal(bare.bare, true);
  assert.equal(bare.main, true);
});

const facts = (over = {}) => ({ defaultBranch: "origin/main", defaultTip: "1".repeat(40), merged: new Set(["main", "feature/x", "old"]), gone: new Set(["feature/x"]), ...over });
const clean = { dirty: false, lastCommit: 1000 };
const probes = (over = {}) => new Map([...parseWorktrees(PORCELAIN).map((w) => [w.path, clean]), ...Object.entries(over)]);
const byBranch = (rows, b) => rows.find((r) => r.wt.branch === b);

test("a merged, clean, closed linked worktree is removable and part of the bulk set", () => {
  const rows = describe(parseWorktrees(PORCELAIN), facts(), probes(), []);
  const r = byBranch(rows, "feature/x");
  assert.equal(r.merged, true);
  assert.equal(r.gone, true);
  assert.equal(r.removable, true);
  assert.equal(r.bulk, true);
  assert.equal(r.why, null);
});

test("main, locked, missing, dirty, unknown and open worktrees are never removable", () => {
  const list = parseWorktrees(PORCELAIN);
  const rows = describe(list, facts(), probes({ "/Users/me/dev/app-feature": { dirty: true, lastCommit: 1 } }), []);
  assert.equal(byBranch(rows, "main").removable, false);
  assert.match(byBranch(rows, "main").why, /main worktree/i);
  assert.equal(byBranch(rows, "feature/x").removable, false);
  assert.equal(byBranch(rows, "feature/x").dirty, true);
  assert.equal(byBranch(rows, "feature/x").bulk, false);
  const old = byBranch(rows, "old");
  assert.equal(old.missing, true);
  assert.equal(old.locked, true);
  assert.equal(old.removable, false);
  assert.equal(byBranch(rows, "spaced").removable, false);

  const unknown = describe(list, facts(), probes({ "/Users/me/dev/app-feature": { dirty: null, lastCommit: null } }), []);
  assert.equal(byBranch(unknown, "feature/x").removable, false);

  const open = describe(list, facts(), probes(), ["/Users/me/dev/app-feature/src"]);
  assert.equal(byBranch(open, "feature/x").inPane, true);
  assert.equal(byBranch(open, "feature/x").removable, false);
});

test("a missing folder that is not locked is prunable, not removable", () => {
  const list = parseWorktrees("worktree /r\nbranch refs/heads/main\n\nworktree /r-x\nbranch refs/heads/x\nprunable gone\n");
  const rows = describe(list, facts(), new Map(), []);
  assert.equal(rows[1].missing, true);
  assert.equal(rows[1].removable, false);
});

test("a branch sitting on the default branch's commit is not called merged", () => {
  const list = parseWorktrees(PORCELAIN);
  const rows = describe(list, facts({ defaultTip: "2".repeat(40) }), probes(), []);
  const r = byBranch(rows, "feature/x");
  assert.equal(r.atDefault, true);
  assert.equal(r.merged, false);
  assert.equal(r.removable, true);
  assert.equal(r.bulk, false);
});

test("a detached head is never merged", () => {
  const rows = describe(parseWorktrees(PORCELAIN), facts(), probes(), []);
  const r = rows.find((x) => x.wt.detached);
  assert.equal(r.merged, false);
  assert.equal(r.removable, true);
  assert.equal(r.bulk, false);
});

test("a pane belongs to the deepest worktree holding its folder", () => {
  const list = parseWorktrees("worktree /r\nbranch refs/heads/main\n\nworktree /r/.wt/a\nbranch refs/heads/a\n");
  const rows = describe(list, facts(), new Map([["/r/.wt/a", clean]]), ["/r/.wt/a/src"]);
  assert.equal(rows[0].inPane, false);
  assert.equal(rows[1].inPane, true);
  assert.equal(contains("/r", "/rx"), false);
  assert.equal(contains("/r/", "/r"), true);
});

test("paths are shortened with ~, gone upstreams are found", () => {
  assert.equal(shortenPath("/Users/me/dev/app"), "~/dev/app");
  assert.equal(shortenPath("/home/me"), "~");
  assert.equal(shortenPath("/Users/mex"), "~");
  assert.equal(shortenPath("/srv/app"), "/srv/app");
  assert.deepEqual([...goneBranches("a\t[gone]\nb\t[ahead 1]\nc\t\nd\t[gone]\n")], ["a", "d"]);
});

test("a repo has worktrees to manage when it has another checkout, or a record whose folder is gone", () => {
  assert.equal(hasWorktrees(parseWorktrees("worktree /r\nHEAD 1\nbranch refs/heads/main\n")), false);
  assert.equal(hasWorktrees(parseWorktrees("worktree /r\nHEAD 1\n\nworktree /r-x\nHEAD 2\n")), true);
  assert.equal(hasWorktrees(parseWorktrees("worktree /r\nHEAD 1\nprunable gitdir file points to non-existent location\n")), true);
  assert.equal(hasWorktrees([]), false);
});

test("in a repo outside your projects nothing says whether a worktree has changes: it can be removed one at a time, never as part of the merged & clean", () => {
  const rows = describe(parseWorktrees(PORCELAIN), facts(), new Map(), [], true);
  const r = byBranch(rows, "feature/x");
  assert.deepEqual([r.dirty, r.unchecked, r.removable, r.merged, r.bulk], [null, true, true, true, false]);
  // What stops a worktree anywhere still stops it, and those aren't called unchecked.
  assert.deepEqual(
    rows.map((row) => [row.removable, row.unchecked]),
    [[false, false], [true, true], [true, true], [false, false], [false, false]],
  );
  assert.ok(rows.every((row) => !row.bulk));
});
