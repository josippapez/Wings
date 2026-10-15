import assert from "node:assert/strict";
import test from "node:test";
import { argsOf } from "./args.ts";

const github = { provider: "github", url: "https://github.com/o/r/pull/5", number: 5, repo: "o/r" };
const gitlab = { provider: "gitlab", url: "https://gitlab.com/g/p/-/merge_requests/5", number: 5, repo: "g/p", remote: { kind: "gitlab", host: "gitlab.com", path: "g/p" } };

test("a tab's args are the PR's fields and, for a provider that needs one, its remote", () => {
  assert.deepEqual(argsOf(github, "Fix", "/work/r"), { ...github, title: "Fix", cwd: "/work/r" });
  assert.deepEqual(argsOf(gitlab, "Fix", "/work/r"), { ...gitlab, title: "Fix", cwd: "/work/r" });
  assert.equal("remote" in argsOf(github, "Fix", "/work/r"), false);
});
