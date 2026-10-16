import assert from "node:assert/strict";
import { test } from "node:test";

import { azureApiOrg, branchLabel, defaultBranch, git, gitOk, gitRun, onGitChange, parseRemote, repoRoot, watchedProject } from "./git.js";

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

test("repoRoot keeps a found root for good and a miss for a minute, and never keeps a refusal", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  let answer = failed();
  const calls = fakeGit(t, (args, cwd) => (cwd === "/work/app/src" ? ok("/work/app\n") : answer));
  assert.deepEqual(await Promise.all([repoRoot("/work/app/src"), repoRoot("/work/app/src")]), ["/work/app", "/work/app"]);
  assert.equal(await repoRoot("/work/app/src"), "/work/app");
  assert.equal(calls.length, 1, "found once, shared and kept");

  assert.equal(await repoRoot("/tmp/plain"), null);
  assert.equal(await repoRoot("/tmp/plain"), null);
  assert.equal(calls.length, 2, "a miss is not asked again within the minute");
  t.mock.timers.tick(61_000);
  answer = ok("/tmp/plain\n");
  assert.equal(await repoRoot("/tmp/plain"), "/tmp/plain", "git init later is seen after the minute");

  answer = new Error("not approved yet");
  assert.equal(await repoRoot("/tmp/other"), null);
  answer = ok("/tmp/other\n");
  assert.equal(await repoRoot("/tmp/other"), "/tmp/other", "a refusal is asked again at once");
  assert.equal(await repoRoot(""), null);
  assert.equal(calls.length, 5);
});

test("forget makes the next repoRoot and defaultBranch ask git again, for one argument or all", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  let root = "/a";
  const calls = fakeGit(t, (args, cwd) => (args[0] === "rev-parse" && args[1] === "--show-toplevel" ? (cwd === "/none" ? failed() : ok(`${root}\n`)) : args[2] === "origin/HEAD" ? ok("origin/main\n") : failed()));
  assert.equal(await repoRoot("/x"), "/a");
  assert.equal(await repoRoot("/none"), null);
  root = "/b";
  assert.equal(await repoRoot("/x"), "/a", "kept");
  repoRoot.forget("/x");
  assert.equal(await repoRoot("/x"), "/b", "forgotten, so a nested repo made since is seen");
  const asked = calls.length;
  repoRoot.forget();
  await repoRoot("/none");
  assert.equal(calls.length, asked + 1, "forgetting all drops a kept miss too");

  await defaultBranch("/r");
  await defaultBranch("/r");
  const before = calls.length;
  defaultBranch.forget("/r");
  await defaultBranch("/r");
  assert.equal(calls.length, before + 1);
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

/** A stand-in `wings` for `onGitChange`: `emit` sends a git event, `focus` changes focus. */
function fakeEvents(projects, focused = true) {
  let gitListener;
  const focusListeners = [];
  const w = {
    focused,
    projects: async () => projects,
    onGitChange: (fn) => ((gitListener = fn), () => (gitListener = undefined)),
    onFocus: (fn) => (focusListeners.push(fn), () => focusListeners.splice(focusListeners.indexOf(fn), 1)),
  };
  return {
    w,
    emit: (...projectIds) => gitListener?.({ projectIds }),
    focus: (next) => {
      w.focused = next;
      focusListeners.forEach((fn) => fn({ focused: next }));
    },
    listening: () => gitListener !== undefined || focusListeners.length > 0,
  };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

test("onGitChange runs nothing while unfocused, then once with the union of ids on focus", async (t) => {
  fakeGit(t, { "rev-parse --show-toplevel": ok("/r\n") });
  const ev = fakeEvents([{ id: "a", path: "/r" }, { id: "b", path: "/s" }], false);
  const runs = [];
  const rootsSeen = [];
  onGitChange((ids, roots) => void (runs.push(ids), rootsSeen.push(roots)), { wings: ev.w });
  ev.emit("a");
  ev.emit("b", "a");
  await tick();
  assert.deepEqual(runs, []);
  ev.focus(true);
  await tick();
  assert.deepEqual(runs, [["a", "b"]]);
  assert.deepEqual(rootsSeen, [["/r"]], "the repo roots of the changed projects, once each");
  ev.focus(false);
  ev.focus(true);
  await tick();
  assert.equal(runs.length, 1);
});

test("onGitChange folds a burst that comes while the handler runs into one more run", async (t) => {
  fakeGit(t, { "rev-parse --show-toplevel": ok("/r\n") });
  const ev = fakeEvents([{ id: "a", path: "/r" }]);
  const runs = [];
  let release;
  onGitChange(
    (ids) => {
      runs.push(ids);
      return runs.length === 1 ? new Promise((r) => (release = r)) : undefined;
    },
    { wings: ev.w },
  );
  ev.emit("a");
  await tick();
  for (let i = 0; i < 5; i++) ev.emit("a");
  await tick();
  assert.equal(runs.length, 1);
  release();
  await tick();
  assert.deepEqual(runs, [["a"], ["a"]]);
});

test("onGitChange makes defaultBranch ask again for the changed repo only, and the stop function removes both listeners", async (t) => {
  const calls = fakeGit(t, (args, cwd) => (args[0] === "rev-parse" && args[1] === "--show-toplevel" ? ok(`${cwd}\n`) : args[1] === "--abbrev-ref" ? ok("origin/main\n") : failed()));
  await defaultBranch("/r");
  await defaultBranch("/s");
  assert.equal(calls.filter((c) => c.includes("--abbrev-ref")).length, 2);
  const ev = fakeEvents([{ id: "a", path: "/r" }, { id: "b", path: "/s" }]);
  const seen = [];
  const stop = onGitChange(async (ids) => {
    seen.push(ids);
    await defaultBranch("/r");
    await defaultBranch("/s");
  }, { wings: ev.w });
  ev.emit("a");
  await tick();
  assert.deepEqual(seen, [["a"]]);
  assert.equal(calls.filter((c) => c.includes("--abbrev-ref")).length, 3);
  stop();
  assert.equal(ev.listening(), false);
  defaultBranch.forget();
  repoRoot.forget();
});

test("watchedProject is the project whose repo the folder is in, and null for a repo no project has", async (t) => {
  fakeGit(t, (args, cwd) => (args[1] === "--show-toplevel" ? (cwd.startsWith("/nested") ? ok("/nested\n") : ok(`${cwd.split("/").slice(0, 2).join("/")}\n`)) : failed()));
  const projects = [{ id: "a", path: "/web" }, { id: "b", path: "/api/server" }];
  assert.equal(await watchedProject("/web/src/deep", projects), "a");
  assert.equal(await watchedProject("/api/x", projects), "b");
  assert.equal(await watchedProject("/nested/sub", projects), null);
  repoRoot.forget();
});

test("watchedProject is null outside any repo", async (t) => {
  fakeGit(t, {});
  assert.equal(await watchedProject("/tmp/x", [{ id: "a", path: "/web" }]), null);
  repoRoot.forget();
});
