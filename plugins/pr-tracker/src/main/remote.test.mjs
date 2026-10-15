import assert from "node:assert/strict";
import test from "node:test";
import { azureApiOrg, parseRemote } from "wings-plugin-sdk/git";
import { forgetHosts, remoteFor, remotes } from "./remote.ts";

// What the tracker relies on `parseRemote` for: which forge a remote is, whatever its URL looks like. The SDK owns the function.
test("remote URLs in every shape name their forge, host and repository", () => {
  const github = { kind: "github", host: "github.com", path: "o/r" };
  for (const url of ["https://github.com/o/r.git", "git@github.com:o/r.git", "ssh://git@github.com/o/r", "https://user:tok@github.com/o/r/", "git://github.com/o/r.git", "ssh://git@ssh.github.com:443/o/r.git"]) assert.deepEqual(parseRemote(url), github, url);
  for (const url of ["git@ghe.corp.example:org/repo.git", "https://ghe.corp.example:8443/org/repo", "ssh://git@ghe.corp.example:2222/org/repo.git", "HTTPS://GHE.Corp.Example/org/repo"]) assert.deepEqual(parseRemote(url), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" }, url);
  assert.deepEqual(parseRemote("ssh://git@gitlab.corp.example:2222/g/s/p"), { kind: "gitlab", host: "gitlab.corp.example", path: "g/s/p" });
  assert.deepEqual(parseRemote("git@github.com:123/repo.git"), { kind: "github", host: "github.com", path: "123/repo" }, "an owner made of digits is a path, not a port");
  for (const url of ["https://bitbucket.org/ws/my-repo.v2.git", "git@bitbucket.org:ws/my-repo.v2.git", "ssh://git@bitbucket.org:22/ws/my-repo.v2"]) assert.deepEqual(parseRemote(url), { kind: "bitbucket", workspace: "ws", repo: "my-repo.v2" }, url);
  assert.equal(parseRemote("https://bitbucket.org/ws/a/b").kind, "none");
  for (const url of ["https://codeberg.org/o/r.git", "git@codeberg.org:o/r.git", "ssh://git@codeberg.org:22/o/r"]) assert.deepEqual(parseRemote(url), { kind: "gitea", host: "codeberg.org", owner: "o", repo: "r" }, url);
  for (const url of ["/Users/me/repo", "../repo", "file:///a/b", ""]) assert.equal(parseRemote(url).kind, "none", url);
});

test("an Azure DevOps remote in any form names its organization at the address a reply goes to", () => {
  for (const url of ["git@ssh.dev.azure.com:v3/org/proj/repo", "https://org@dev.azure.com/org/proj/_git/repo", "https://org.visualstudio.com/DefaultCollection/proj/_git/repo", "https://org@org.visualstudio.com/proj/_git/repo"]) {
    assert.equal(azureApiOrg(parseRemote(url).org), "https://dev.azure.com/org", url);
  }
  assert.deepEqual(parseRemote("git@ssh.dev.azure.com:v3/org/My%20Project/repo"), { kind: "azure", org: "https://dev.azure.com/org", project: "My Project", repo: "repo" });
});

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

test("a host no CLI knows stays unresolved without asking the CLIs again for half an hour, or until they are forgotten", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  const calls = fakeWings(t, { origins: { "/x": "git@nobody.example:o/r.git" } });
  assert.deepEqual(await remoteFor("/x"), { kind: "none", host: "nobody.example" });
  const asked = calls.length;
  t.mock.timers.tick(5 * 60_000);
  assert.deepEqual(await remoteFor("/x"), { kind: "none", host: "nobody.example" });
  assert.equal(calls.length, asked, "five minutes on, still not asked");
  forgetHosts();
  await remoteFor("/x");
  assert.ok(calls.length > asked, "Try again looks at the CLIs' hosts once more");
  const again = calls.length;
  t.mock.timers.tick(31 * 60_000);
  await remoteFor("/x");
  assert.ok(calls.length > again, "half an hour on, they are asked again");
});

test("two clones of one unresolved repository share the answer, however the URL was written", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  const calls = fakeWings(t, { origins: { "/1": "git@nobody.example:o/r.git", "/2": "https://nobody.example/o/r" } });
  await remoteFor("/1");
  const asked = calls.filter((p) => p !== "git").length;
  t.mock.timers.tick(5 * 60_000);
  assert.deepEqual(await remoteFor("/2"), { kind: "none", host: "nobody.example" });
  assert.equal(calls.filter((p) => p !== "git").length, asked);
});

test("the CLIs' hosts are read once a minute, however many remotes ask", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  const calls = fakeWings(t, { origins: { "/a": "git@a.example:o/r.git", "/b": "git@b.example:o/r.git", "/c": "git@c.example:o/r.git" } });
  await remoteFor("/a");
  t.mock.timers.tick(10_000);
  await remoteFor("/b");
  assert.equal(calls.filter((p) => p === "gh").length, 1);
  t.mock.timers.tick(61_000);
  await remoteFor("/c");
  assert.equal(calls.filter((p) => p === "gh").length, 2);
});
