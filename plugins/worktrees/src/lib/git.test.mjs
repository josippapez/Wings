import assert from "node:assert/strict";
import { test } from "node:test";

import { removeWorktrees } from "./git.ts";

const ok = (stdout = "") => ({ code: 0, stdout, stderr: "" });
const refuse = (stderr) => ({ code: 1, stdout: "", stderr });

/** A fake git where `/r-x` (branch `x`) is merged and clean, so removing it is offered; `more` adds or replaces answers. */
function fakeGit(more) {
  const answers = {
    "worktree list --porcelain": ok("worktree /r\nHEAD 1\nbranch refs/heads/main\n\nworktree /r-x\nHEAD 2\nbranch refs/heads/x\n"),
    "rev-parse --abbrev-ref origin/HEAD": ok("origin/main"),
    "rev-parse --verify --quiet origin/main^{commit}": ok("1"),
    "branch --list --merged=origin/main --format=%(refname:short)": ok("main\nx"),
    "branch --list --format=%(refname:short)%09%(upstream:track)": ok(""),
    "status --porcelain": ok(),
    "log --max-count=1 --format=%ct": ok("1000"),
    ...more,
  };
  globalThis.wings = { exec: async (_program, args) => answers[args.join(" ")] ?? refuse(`fake git has no answer for ${args.join(" ")}`) };
}

const remove = () => removeWorktrees("/r", ["/r-x"], { deleteBranch: true, onlyMerged: true }, () => []);

test("when git refuses to remove a worktree, the page shows its first line, not the advice after it", async () => {
  fakeGit({ "worktree remove -- /r-x": refuse("fatal: cannot remove a locked working tree, lock reason: busy\nuse 'remove -f -f' to override or unlock first\n") });
  const out = await remove();
  assert.deepEqual(out.failed, [{ path: "/r-x", error: "fatal: cannot remove a locked working tree, lock reason: busy" }]);
  assert.deepEqual(out.removed, []);
});

test("when git refuses to delete the branch, the worktree is still removed and the page shows its first line", async () => {
  fakeGit({
    "worktree remove -- /r-x": ok(),
    "branch -d x": refuse("error: the branch 'x' is not fully merged\nhint: If you are sure you want to delete it, run 'git branch -D x'\n"),
  });
  const out = await remove();
  assert.deepEqual(out.removed, ["/r-x"]);
  assert.deepEqual(out.branchErrors, [{ branch: "x", error: "error: the branch 'x' is not fully merged" }]);
});
