import assert from "node:assert/strict";
import test from "node:test";
import { PROVIDERS } from "../providers.ts";
import { forgeFor, forgeOf, forges, isKnown, linkOf } from "./registry.ts";

const github = (host = "github.com") => ({ kind: "github", host, path: "o/r" });
const gitlab = (host = "gitlab.com") => ({ kind: "gitlab", host, path: "g/p" });
const azure = { kind: "azure", org: "https://dev.azure.com/acme", project: "My Project", repo: "app" };
const bitbucket = { kind: "bitbucket", workspace: "w", repo: "r" };
const gitea = { kind: "gitea", host: "codeberg.org", owner: "o", repo: "r" };

test("every provider has a forge, and a link, a remote or a tab's args find their own", () => {
  assert.deepEqual(Object.keys(forges).sort(), Object.keys(PROVIDERS).sort());
  assert.equal(forgeOf({ provider: "gitlab" }), forges.gitlab);
  for (const remote of [github(), gitlab(), azure, bitbucket, gitea]) assert.equal(forgeFor(remote), forges[remote.kind], remote.kind);
  assert.deepEqual([github(), gitlab(), azure, bitbucket, gitea].map(isKnown), [true, true, true, true, true]);
  assert.deepEqual([{ kind: "none" }, { kind: "none", host: "git.corp.example" }, { kind: "unknown", host: "git.corp.example", path: "o/r" }].map(isKnown), [false, false, false]);
});

test("a sign-in problem is kept per host, so github.com and an Enterprise host don't share one", () => {
  const key = (remote) => forgeFor(remote).problemKey(remote);
  assert.notEqual(key(github()), key(github("ghe.example")));
  assert.notEqual(key(github("x")), key(gitlab("x")));
  assert.notEqual(key(gitlab()), key(gitlab("gitlab.corp.example")));
  assert.deepEqual([key(azure), key(bitbucket), key(gitea)], ["https://dev.azure.com/acme", "bitbucket", "gitea:codeberg.org"]);
});

test("what is kept per repository is named by the repository, and never mixes two", () => {
  const key = (remote) => forgeFor(remote).repoKey(remote);
  assert.equal(key(github()), key(github()));
  assert.notEqual(key(github()), key({ ...github(), path: "o/other" }));
  assert.notEqual(key(github("x")), key(gitlab("x")));
  const all = [github(), gitlab(), azure, bitbucket, gitea].map(key);
  assert.equal(new Set(all).size, all.length);
});

test("a tab's args become the link of their provider, and a provider that needs its remote says when the args have none", async () => {
  const base = { url: "https://example.com/pr/5", number: 5, repo: "o/r", title: "t", cwd: "/work/r" };
  assert.deepEqual(await linkOf({ ...base, provider: "github" }), { provider: "github", url: base.url, number: 5, repo: "o/r" });
  assert.deepEqual(await linkOf({ ...base, provider: "gitlab", remote: gitlab() }), { provider: "gitlab", url: base.url, number: 5, repo: "o/r", remote: gitlab() });
  assert.deepEqual(await linkOf({ ...base, provider: "bitbucket", remote: bitbucket }), { provider: "bitbucket", url: base.url, number: 5, repo: "o/r", remote: bitbucket, cwd: "/work/r" });
  assert.deepEqual(await linkOf({ ...base, provider: "gitea", remote: gitea }), { provider: "gitea", url: base.url, number: 5, repo: "o/r", remote: gitea });
  for (const [provider, remote, name] of [["gitlab", undefined, /GitLab/], ["bitbucket", gitlab(), /Bitbucket/], ["gitea", bitbucket, /Codeberg/], ["azure", undefined, /Azure/]]) {
    await assert.rejects(linkOf({ ...base, provider, remote }), name, provider);
  }
});

test("only a forge with something to approve with, vote with or read pushes from says so", () => {
  assert.deepEqual(Object.entries(forges).filter(([, f]) => f.approve).map(([k]) => k).sort(), ["bitbucket", "gitea", "github", "gitlab"]);
  assert.deepEqual(Object.entries(forges).filter(([, f]) => f.vote || f.pushes || f.snippet).map(([k]) => k), ["azure"]);
  assert.deepEqual(Object.entries(forges).filter(([, f]) => f.login).map(([k]) => k).sort(), ["azure", "github", "gitlab"]);
});
