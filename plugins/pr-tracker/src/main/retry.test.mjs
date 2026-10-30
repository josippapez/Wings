import assert from "node:assert/strict";
import test from "node:test";

// A `wings.exec` for gh and glab that says who you are and lists no PRs, and logs what it was asked. The CLI helper takes
// `wings` when it is made, as it does in a plugin, so this is in place before the modules load.
const calls = [];
globalThis.wings = {
  exec: async (program, args) => {
    calls.push(`${program} ${args.join(" ")}`);
    if (program === "gh") return { code: 0, stdout: args[0] === "api" ? "me\n" : "[]", stderr: "" };
    if (program === "glab") return { code: 0, stdout: args.includes("user") ? JSON.stringify({ username: "me" }) : "[]", stderr: "" };
    if (program === "az") return { code: 0, stdout: args[0] === "account" ? "me@contoso.example\n" : "[]", stderr: "" };
    return { code: 0, stdout: "", stderr: "" };
  },
  // The GitHub list is one GraphQL query that every repo asked at once shares, which `gh api graphql` takes as a post.
  post: async (program, url) => {
    calls.push(`${program} post ${url}`);
    return { code: 0, stdout: JSON.stringify({ data: { r0: { pullRequests: { nodes: [] } }, asked: { nodes: [] } } }), stderr: "" };
  },
  setBadge: async () => {},
  // Try again also reloads the open tabs' queries.
  broadcast: async () => {},
  onBroadcast: () => () => {},
};
const { forges } = await import("../lib/forges/registry.ts");
const { lookups } = await import("./links.ts");
const { connectClis, retry } = await import("./retry.ts");
const { diffs, failures, status } = await import("./state.ts");
connectClis();

const github = { kind: "github", host: "github.com", path: "o/r" };
const gitlab = { kind: "gitlab", host: "gitlab.com", path: "g/p" };
const azure = { kind: "azure", org: "https://dev.azure.com/contoso", project: "P", repo: "R" };
const asked = (what) => calls.filter((c) => c.includes(what)).length;
const url = "https://github.com/o/r/pull/1";

test("Try again forgets what the tracker kept: statuses, diffs, failed checks, lookups, and who gh, glab and az are signed in as", async () => {
  const listBoth = async () => {
    await forges.github.list("/r", github);
    await forges.gitlab.list("/g", gitlab);
    await forges.azure.list("/a", azure);
    await forges.azure.me(azure);
  };
  const lookup = () => lookups.get({ cwd: "/r", remote: github, branch: "feat" });

  await listBoth();
  await listBoth();
  await lookup();
  await lookup();
  assert.deepEqual([asked("api user"), asked("hostname gitlab.com user"), asked("--head feat"), asked("post graphql"), asked("account show")], [1, 1, 1, 1, 1], "each is asked once and kept");
  status.set(url, { error: "x" });
  diffs.set(url, { key: "k", patch: "", comments: [] });
  failures.set(url, { key: "k", items: Promise.resolve([]), list: [] });

  await retry();
  assert.deepEqual([status.size, diffs.size, failures.size], [0, 0, 0]);
  await listBoth();
  await lookup();
  assert.deepEqual([asked("api user"), asked("hostname gitlab.com user"), asked("--head feat"), asked("post graphql"), asked("account show")], [2, 2, 2, 2, 2], "each is asked again");
});

test("signing in to one CLI forgets everything too, since its badge is for every pane", async () => {
  status.set(url, { error: "x" });
  await forges.gitlab.login.cli.retry();
  assert.equal(status.size, 0);
});
