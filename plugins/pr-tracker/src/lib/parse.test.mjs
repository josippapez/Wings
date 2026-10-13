import assert from "node:assert/strict";
import test from "node:test";
import { parseRemote, splitRemote } from "./parse.ts";

test("github.com in every URL shape", () => {
  const want = { kind: "github", host: "github.com", path: "o/r" };
  for (const url of ["https://github.com/o/r.git", "https://github.com/o/r", "https://github.com/o/r/", "git@github.com:o/r.git", "ssh://git@github.com/o/r.git", "https://user:tok@github.com/o/r.git", "git://github.com/o/r.git", "ssh://git@ssh.github.com:443/o/r.git"]) {
    assert.deepEqual(parseRemote(url), want, url);
  }
});

test("an unnamed host is unknown until a CLI knows it, whatever the port", () => {
  assert.deepEqual(parseRemote("git@ghe.corp.example:org/repo.git"), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" });
  assert.deepEqual(parseRemote("https://ghe.corp.example/org/repo"), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" });
  assert.deepEqual(parseRemote("ssh://git@ghe.corp.example:2222/org/repo.git"), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" });
  assert.deepEqual(parseRemote("https://ghe.corp.example:8443/org/repo.git"), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" });
  assert.deepEqual(parseRemote("HTTPS://GHE.Corp.Example/org/repo"), { kind: "unknown", host: "ghe.corp.example", path: "org/repo" });
});

test("GitLab: gitlab.com and hosts named gitlab, with subgroups", () => {
  assert.deepEqual(parseRemote("git@gitlab.com:g/s/p.git"), { kind: "gitlab", host: "gitlab.com", path: "g/s/p" });
  assert.deepEqual(parseRemote("https://gitlab.corp.example:8443/g/p.git"), { kind: "gitlab", host: "gitlab.corp.example", path: "g/p" });
  assert.deepEqual(parseRemote("ssh://git@gitlab.corp.example:2222/g/s/p"), { kind: "gitlab", host: "gitlab.corp.example", path: "g/s/p" });
});

test("Azure DevOps shapes still parse", () => {
  const want = { kind: "azure", org: "https://dev.azure.com/org", project: "My Project", repo: "repo" };
  assert.deepEqual(parseRemote("git@ssh.dev.azure.com:v3/org/My%20Project/repo"), want);
  assert.deepEqual(parseRemote("https://org@dev.azure.com/org/My%20Project/_git/repo"), want);
  assert.deepEqual(parseRemote("https://org.visualstudio.com/DefaultCollection/proj/_git/repo"), { kind: "azure", org: "https://org.visualstudio.com", project: "proj", repo: "repo" });
});

test("Bitbucket Cloud in https, ssh and scp-style, and only as workspace/repo", () => {
  const want = { kind: "bitbucket", workspace: "ws", repo: "my-repo.v2" };
  for (const url of ["https://bitbucket.org/ws/my-repo.v2.git", "https://user@bitbucket.org/ws/my-repo.v2.git", "git@bitbucket.org:ws/my-repo.v2.git", "ssh://git@bitbucket.org/ws/my-repo.v2.git", "ssh://git@bitbucket.org:22/ws/my-repo.v2"]) {
    assert.deepEqual(parseRemote(url), want, url);
  }
  assert.equal(parseRemote("https://bitbucket.org/ws").kind, "none");
  assert.equal(parseRemote("https://bitbucket.org/ws/a/b").kind, "none");
});

test("Codeberg, in https, ssh and scp-style", () => {
  const want = { kind: "gitea", host: "codeberg.org", owner: "o", repo: "r" };
  for (const url of ["https://codeberg.org/o/r.git", "git@codeberg.org:o/r.git", "ssh://git@codeberg.org/o/r.git", "ssh://git@codeberg.org:22/o/r"]) {
    assert.deepEqual(parseRemote(url), want, url);
  }
});

test("an owner made of digits is still a path, not a port", () => {
  assert.deepEqual(parseRemote("git@github.com:123/repo.git"), { kind: "github", host: "github.com", path: "123/repo" });
  assert.deepEqual(parseRemote("git@ghe.corp.example:2222/repo"), { kind: "unknown", host: "ghe.corp.example", path: "2222/repo" });
});

test("local paths and file URLs name no host", () => {
  for (const url of ["/Users/me/repo", "../repo", "file:///a/b", ""]) assert.equal(parseRemote(url).kind, "none", url);
  assert.equal(splitRemote("/Users/me/repo"), null);
});
