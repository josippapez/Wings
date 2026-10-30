import assert from "node:assert/strict";
import { test } from "node:test";

import { cli } from "./cli.js";
import { githubPulls } from "./github.js";

const GH = { signedOut: /gh auth login/, signIn: [] };

test("githubPulls reads every repository of a turn and the review search in one gh post, and a repository that can't be read fails alone", async () => {
  const posts = [];
  const wings = {
    post: async (program, url, fields, options) => {
      posts.push({ program, url, query: fields.query, options });
      const data = {
        r0: { pullRequests: { nodes: [{ number: 1 }, { number: 2 }] } },
        r1: null,
        r2: { pullRequests: { nodes: [{ number: 3 }] } },
        asked: { nodes: [{ number: 2, repository: { nameWithOwner: "Octo/Repo" } }, { number: 3, repository: { nameWithOwner: "octo/other" } }, {}] },
      };
      // `gh api` prints the answer and exits 1 when it has errors.
      return { code: 1, stdout: JSON.stringify({ data, errors: [{ path: ["r1"], message: "Could not resolve to a Repository" }] }), stderr: "gh: Could not resolve to a Repository" };
    },
  };
  const pulls = githubPulls(cli("gh", { ...GH, wings }), { selection: "number", key: "github" });
  const [a, b, c] = await Promise.allSettled([pulls.get({ owner: "octo", name: "repo" }), pulls.get({ owner: "octo", name: "gone" }), pulls.get({ owner: "octo", name: "other" })]);
  assert.equal(posts.length, 1);
  assert.deepEqual([posts[0].program, posts[0].url, posts[0].options], ["gh", "graphql", undefined]);
  assert.match(posts[0].query, /r0: repository\(owner: "octo", name: "repo"\)/);
  assert.deepEqual([a.value.pulls.length, a.value.asked, c.value.asked], [2, [2], [3]], "the search is split by repository, whatever the case");
  assert.equal(b.reason.message, "Could not resolve to a Repository");
});

test("githubPulls names an Enterprise host to wings.post and turns a signed-out gh into the CLI's problem", async () => {
  let host;
  const wings = {
    post: async (_program, _url, _fields, options) => {
      host = options?.host;
      return { code: 1, stdout: "", stderr: "To get started with GitHub CLI, please run:  gh auth login" };
    },
  };
  const gh = cli("gh", { ...GH, wings });
  await assert.rejects(githubPulls(gh, { selection: "number", host: "ghe.example.com", key: "github:ghe.example.com" }).get({ owner: "o", name: "r" }), { kind: "signed-out" });
  assert.equal(host, "ghe.example.com");
  assert.equal(gh.problem("github:ghe.example.com").kind, "signed-out");
});
