import assert from "node:assert/strict";
import { test } from "node:test";

import { azureApiOrg, branchLabel, defaultBranch, git, gitOk, gitRun, parseRemote, repoRoot } from "./git.js";

const ok = (stdout) => ({ code: 0, stdout, stderr: "" });
const failed = (stderr = "fatal: no", code = 128) => ({ code, stdout: "", stderr });

/** A `wings.exec` that answers `git <args>` from `replies` (a function of the args and cwd, or a map by args), and logs the calls. */
function fakeGit(t, replies) {
  const calls = [];
  globalThis.wings = {
    exec: async (program, args, { cwd }) => {
      calls.push(`${cwd}: ${args.join(" ")}`);
      const reply = typeof replies === "function" ? replies(args, cwd) : replies[args.join(" ")];
      if (reply instanceof Error) throw reply;
      return reply ?? failed();
    },
  };
  t.after(() => delete globalThis.wings);
  return calls;
}

test("git trims what git printed, unless raw asks for it exactly", async (t) => {
  const calls = fakeGit(t, { "branch --show-current": ok("main\n"), diff: ok("diff --git a b\n+x\n") });
  assert.equal(await git("/r", ["branch", "--show-current"]), "main");
  assert.equal(await git("/r", ["diff"], { raw: true }), "diff --git a b\n+x\n");
  assert.deepEqual(calls, ["/r: branch --show-current", "/r: diff"]);
});

test("git throws the first line of stderr after ssh's ** warnings, else names the command", async (t) => {
  const warning = "** WARNING: connection is not using a post-quantum key exchange algorithm.\n** The server may need to be upgraded.\n";
  fakeGit(t, { fetch: failed(`${warning}git@host.example: Permission denied (publickey).\nfatal: Could not read from remote repository.\n`), "merge-base a b": failed("", 1) });
  await assert.rejects(git("/r", ["fetch"]), { message: "git@host.example: Permission denied (publickey)." });
  await assert.rejects(git("/r", ["merge-base", "a", "b"]), { message: "git merge-base failed" });
});

test("gitOk is null for a failed command and for a refused call, and gitRun passes the exit code through", async (t) => {
  fakeGit(t, { "rev-parse HEAD": ok(" abc123\n"), "rev-parse nope": failed(), "grep x": failed("", 1), status: new Error("changes may not run git status") });
  assert.equal(await gitOk("/r", ["rev-parse", "HEAD"]), "abc123");
  assert.equal(await gitOk("/r", ["rev-parse", "nope"]), null);
  assert.equal(await gitOk("/r", ["status"]), null);
  assert.deepEqual(await gitRun("/r", ["grep", "x"]), failed("", 1));
});

test("repoRoot keeps a found root, and asks again after a miss or a refusal", async (t) => {
  let answer = failed();
  const calls = fakeGit(t, (args, cwd) => (cwd === "/work/app/src" ? ok("/work/app\n") : answer));
  assert.deepEqual(await Promise.all([repoRoot("/work/app/src"), repoRoot("/work/app/src")]), ["/work/app", "/work/app"]);
  assert.equal(await repoRoot("/work/app/src"), "/work/app");
  assert.equal(calls.length, 1, "found once, shared and kept");

  assert.equal(await repoRoot("/tmp/plain"), null);
  answer = new Error("not approved yet");
  assert.equal(await repoRoot("/tmp/plain"), null);
  answer = ok("/tmp/plain\n");
  assert.equal(await repoRoot("/tmp/plain"), "/tmp/plain", "git init later is seen");
  assert.equal(await repoRoot(""), null);
  assert.equal(calls.length, 4);
});

test("defaultBranch is origin/HEAD, else the first of the usual names, kept once found", async (t) => {
  const has = new Set(["origin/master"]);
  const calls = fakeGit(t, (args, cwd) => {
    if (cwd === "/r1") return args[2] === "origin/HEAD" ? ok("origin/develop\n") : failed();
    return args[1] === "--verify" && has.has(args[3].replace("^{commit}", "")) ? ok("abc\n") : failed();
  });
  assert.equal(await defaultBranch("/r1"), "origin/develop");
  assert.equal(await defaultBranch("/r1"), "origin/develop");
  assert.equal(calls.length, 1);

  assert.equal(await defaultBranch("/r2"), "origin/master");
  assert.deepEqual(calls.slice(1).map((c) => c.split(": ")[1]), ["rev-parse --abbrev-ref origin/HEAD", "rev-parse --verify --quiet origin/main^{commit}", "rev-parse --verify --quiet origin/master^{commit}"]);

  has.clear();
  assert.equal(await defaultBranch("/r3"), null);
  has.add("main");
  assert.equal(await defaultBranch("/r3"), "main", "a miss isn't kept");
});

test("branchLabel names a branch, a detached head or an empty repo", () => {
  assert.deepEqual([branchLabel("main", "abcdef1234"), branchLabel("", "abcdef1234"), branchLabel("", null)], ["main", "detached at abcdef1", "no commits"]);
});

test("parseRemote names the forge, and reads every Azure form", () => {
  const azure = { kind: "azure", org: "https://dev.azure.com/acme", project: "My Project", repo: "app" };
  assert.deepEqual(parseRemote("git@ssh.dev.azure.com:v3/acme/My%20Project/app"), azure);
  assert.deepEqual(parseRemote("https://acme@dev.azure.com/acme/My%20Project/_git/app\n"), azure);
  assert.deepEqual(parseRemote("https://acme.visualstudio.com/DefaultCollection/proj/_git/app"), { kind: "azure", org: "https://acme.visualstudio.com", project: "proj", repo: "app" });
  // The older ssh host, which Microsoft's docs give as a repository's `sshUrl` like this, and say still works.
  assert.deepEqual(parseRemote("acme@vs-ssh.visualstudio.com:v3/acme/My%20Project/app"), azure);
  for (const url of ["https://github.com/o/r.git", "git@github.com:o/r.git", "ssh://git@ssh.github.com:443/o/r"]) {
    assert.deepEqual(parseRemote(url), { kind: "github", host: "github.com", path: "o/r" }, url);
  }
  assert.deepEqual(parseRemote("git@gitlab.example.com:g/s/p.git"), { kind: "gitlab", host: "gitlab.example.com", path: "g/s/p" });
  assert.deepEqual(parseRemote("git@bitbucket.org:ws/repo.git"), { kind: "bitbucket", workspace: "ws", repo: "repo" });
  assert.deepEqual(parseRemote("https://codeberg.org/o/r.git"), { kind: "gitea", host: "codeberg.org", owner: "o", repo: "r" });
  assert.deepEqual(parseRemote("git@git.corp.example:org/repo.git"), { kind: "unknown", host: "git.corp.example", path: "org/repo" });
  assert.deepEqual(parseRemote("https://bitbucket.org/ws"), { kind: "none", host: "bitbucket.org" });
  for (const url of ["/Users/me/repo", "file:///a/b", ""]) assert.deepEqual(parseRemote(url), { kind: "none" }, url);
});

test("azureApiOrg gives an organization its dev.azure.com address, whichever form its remote had", () => {
  for (const url of ["git@ssh.dev.azure.com:v3/acme/proj/app", "acme@vs-ssh.visualstudio.com:v3/acme/proj/app", "https://acme@dev.azure.com/acme/proj/_git/app", "https://acme.visualstudio.com/DefaultCollection/proj/_git/app", "https://acme@acme.visualstudio.com/proj/_git/app"]) {
    assert.equal(azureApiOrg(parseRemote(url).org), "https://dev.azure.com/acme", url);
  }
});
