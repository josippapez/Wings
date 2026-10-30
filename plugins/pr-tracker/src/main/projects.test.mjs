import assert from "node:assert/strict";
import test from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

// Ten GitHub projects whose `origin` is read at different speeds, like git calls through the host are.
const projects = Array.from({ length: 10 }, (_, i) => ({ id: `p${i}`, name: `p${i}`, path: `/work/r${i}` }));
const calls = [];
const sections = new Map();
const node = (n) => ({ number: n, title: `PR ${n}`, state: "OPEN", isDraft: false, reviewDecision: null, mergeable: "MERGEABLE", additions: 1, deletions: 1, changedFiles: 1, headRefName: `f${n}`, baseRefName: "main", url: `https://github.com/acme/r0/pull/${n}`, updatedAt: "t", author: { login: "me" }, comments: { totalCount: 0 }, reviews: { totalCount: 0 }, statusCheckRollup: null });
globalThis.wings = {
  focused: true,
  onFocus: () => () => {},
  projects: async () => projects,
  storage: { get: async () => null, set: async () => {} },
  setProjectSection: async (id, _section, rows) => void sections.set(id, rows),
  notify: async () => {},
  setBadge: async () => {},
  broadcast: async () => {},
  onBroadcast: () => () => {},
  exec: async (program, args, { cwd } = {}) => {
    calls.push(`${program} ${args.join(" ")}`);
    if (program === "git") {
      await sleep(Number(cwd.slice(-1)) * 3);
      return { code: 0, stdout: `git@github.com:acme/${cwd.split("/").at(-1)}.git\n`, stderr: "" };
    }
    return { code: 0, stdout: args[0] === "api" ? "me\n" : "{}", stderr: "" };
  },
  post: async (program, url, fields) => {
    calls.push(`${program} post ${url}`);
    const data = { asked: { nodes: [] } };
    for (const [i, repo] of [...fields.query.matchAll(/name: "(r\d)"/g)].entries()) data[`r${i}`] = { pullRequests: { nodes: repo[1] === "r0" ? [node(1)] : [] } };
    return { code: 0, stdout: JSON.stringify({ data }), stderr: "" };
  },
};
const { refreshProjects } = await import("./projects.ts");
const { status } = await import("./state.ts");
const { forges } = await import("../lib/forges/registry.ts");

test("the sidebar lists of ten GitHub projects are one gh call, and a pane's status for a listed PR costs none", async () => {
  await refreshProjects(true);
  assert.equal(calls.filter((c) => c.startsWith("gh post graphql")).length, 1, "one query for ten repos");
  assert.equal(calls.filter((c) => c.startsWith("gh pr")).length, 0);
  assert.equal(sections.get("p0").length, 1);
  assert.equal(status.has("https://github.com/acme/r0/pull/1"), true, "the list keeps the full status of the PR");

  const link = { provider: "github", url: "https://github.com/acme/r0/pull/1", number: 1, repo: "acme/r0" };
  const before = calls.length;
  assert.equal((await forges.github.status(link)).label, "Open");
  assert.equal(calls.length, before, "served from the list the poll just read");
  await forges.github.status(link, { fresh: true });
  assert.equal(calls.length, before + 1, "Refresh reads again, as one call");
  assert.match(calls.at(-1), /^gh post graphql/);
});
