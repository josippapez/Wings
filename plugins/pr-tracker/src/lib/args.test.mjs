import assert from "node:assert/strict";
import test from "node:test";
import { argsOf, readArgs } from "./args.ts";

const github = { provider: "github", url: "https://github.com/o/r/pull/5", number: 5, repo: "o/r" };
const gitlab = { provider: "gitlab", url: "https://gitlab.com/g/p/-/merge_requests/5", number: 5, repo: "g/p", remote: { kind: "gitlab", host: "gitlab.com", path: "g/p" } };

test("a tab's args are the PR's fields and, for a provider that needs one, its remote", () => {
  assert.deepEqual(argsOf(github, "Fix", "/work/r"), { ...github, title: "Fix", cwd: "/work/r" });
  assert.deepEqual(argsOf(gitlab, "Fix", "/work/r"), { ...gitlab, title: "Fix", cwd: "/work/r" });
  assert.equal("remote" in argsOf(github, "Fix", "/work/r"), false);
});

test("what a tab saved reads back as the same args, whatever JSON did to it in between", () => {
  for (const link of [github, gitlab]) {
    const args = argsOf(link, "Fix", "/work/r");
    assert.deepEqual(readArgs(JSON.parse(JSON.stringify(args))), args);
  }
});

test("args that aren't a pull request's are refused, so a tab opened with something else lists the project's PRs", () => {
  const ok = argsOf(github, "Fix", "/work/r");
  for (const bad of [null, undefined, "x", 5, [], {}, { ...ok, provider: "svn" }, { ...ok, provider: "toString" }, { ...ok, number: "5" }, { ...ok, url: 5 }, { ...ok, title: undefined }, { ...ok, remote: "gitlab" }, { ...ok, remote: {} }]) {
    assert.equal(readArgs(bad), null, JSON.stringify(bad));
  }
});
