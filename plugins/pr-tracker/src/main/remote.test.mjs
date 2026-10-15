import assert from "node:assert/strict";
import test from "node:test";
import { forgetHosts, remoteFor, remotes } from "./remote.ts";

/** A `wings.exec` that answers git's `origin` per folder, and which hosts `gh` and `glab` are signed in to. */
function fakeWings(t, { origins, gh = [], glab = [] }) {
  const calls = [];
  globalThis.wings = {
    exec: async (program, args, { cwd } = {}) => {
      calls.push(program);
      if (program === "git") return origins[cwd] ? { code: 0, stdout: `${origins[cwd]}\n`, stderr: "" } : { code: 128, stdout: "", stderr: "fatal: no such remote" };
      if (program === "gh") return { code: 0, stdout: JSON.stringify({ hosts: Object.fromEntries(gh.map((host) => [host, []])) }), stderr: "" };
      return { code: 0, stdout: "", stderr: glab.map((host) => `\x1b[1m${host}\x1b[0m`).join("\n") };
    },
  };
  t.after(() => {
    delete globalThis.wings;
    remotes.clear();
    forgetHosts();
  });
  return calls;
}

test("a folder's remote is its origin, and a folder with none is not asked again", async (t) => {
  const calls = fakeWings(t, { origins: { "/a": "git@github.com:o/r.git" } });
  assert.deepEqual(await remoteFor("/a"), { kind: "github", host: "github.com", path: "o/r" });
  assert.deepEqual(await remoteFor("/none"), { kind: "none" });
  assert.deepEqual(await remoteFor("/none"), { kind: "none" });
  assert.equal(calls.filter((p) => p === "git").length, 2, "one git call per folder");
});

test("a host with no telling name is GitHub or GitLab once the CLI is signed in to it", async (t) => {
  fakeWings(t, { origins: { "/g": "git@ghe.corp.example:org/repo.git", "/l": "https://git.corp.example/g/sub/p.git" }, gh: ["ghe.corp.example"], glab: ["git.corp.example"] });
  assert.deepEqual(await remoteFor("/g"), { kind: "github", host: "ghe.corp.example", path: "org/repo" });
  assert.deepEqual(await remoteFor("/l"), { kind: "gitlab", host: "git.corp.example", path: "g/sub/p" });
  assert.equal(remotes.get("/g").kind, "github", "kept, so a pane's badge can find its host's problems");
});

test("a host no CLI knows stays unresolved without asking the CLIs again, until they are forgotten", async (t) => {
  const calls = fakeWings(t, { origins: { "/x": "git@nobody.example:o/r.git" } });
  assert.deepEqual(await remoteFor("/x"), { kind: "none", host: "nobody.example" });
  const asked = calls.length;
  assert.deepEqual(await remoteFor("/x"), { kind: "none", host: "nobody.example" });
  assert.equal(calls.length, asked);
  forgetHosts();
  await remoteFor("/x");
  assert.ok(calls.length > asked, "Try again looks at the CLIs' hosts once more");
});

test("two clones of one unresolved repository share the answer, however the URL was written", async (t) => {
  const calls = fakeWings(t, { origins: { "/1": "git@nobody.example:o/r.git", "/2": "https://nobody.example/o/r" } });
  await remoteFor("/1");
  const asked = calls.filter((p) => p !== "git").length;
  assert.deepEqual(await remoteFor("/2"), { kind: "none", host: "nobody.example" });
  assert.equal(calls.filter((p) => p !== "git").length, asked);
});
