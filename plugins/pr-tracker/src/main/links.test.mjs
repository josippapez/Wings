import assert from "node:assert/strict";
import test from "node:test";

// The 60 s lookup reuses the branch read last, and only a repo no project has is polled for its branch.
const calls = [];
const ok = (stdout) => ({ code: 0, stdout, stderr: "" });
globalThis.wings = new Proxy(
  {
    focused: false,
    projects: async () => [{ id: "p1", path: "/a", name: "a" }],
    setBadge: async () => {},
    async exec(program, args, { cwd } = {}) {
      calls.push([program, ...args].join(" "));
      if (program === "gh") return ok(JSON.stringify([{ url: "https://github.com/o/r/pull/1", number: 1 }]));
      if (args[0] === "remote") return ok("git@github.com:o/r.git");
      if (args[0] === "rev-parse") return ok(cwd);
      if (args[0] === "branch") return ok("feat");
      return ok("0 1");
    },
  },
  { get: (t, k) => (k in t ? t[k] : () => () => {}) },
);

const { refreshLinks, projectOf } = await import("./links.ts");
const { state, links } = await import("./state.ts");
const count = (prefix) => calls.filter((c) => c.startsWith(prefix)).length;

test("a lookup without local reads runs no branch or rev-list, and a pane's project is known after the first read", async () => {
  state.panes = [
    { paneId: "in", cwd: "/a", title: "", session: null },
    { paneId: "out", cwd: "/b", title: "", session: null },
  ];
  await refreshLinks();
  assert.equal(projectOf(state.panes[0]), "p1");
  assert.equal(projectOf(state.panes[1]), null, "no project has /b's repo");
  assert.equal(links.get("in")?.url, "https://github.com/o/r/pull/1");
  const [branches, counts] = [count("git branch"), count("git rev-list")];
  assert.deepEqual([branches, counts], [2, 2]);

  await refreshLinks({ local: false });
  assert.deepEqual([count("git branch"), count("git rev-list")], [2, 2], "the 60 s cycle runs no git");

  await refreshLinks({ which: (pane) => projectOf(pane) === null });
  assert.deepEqual([count("git branch"), count("git rev-list")], [3, 3], "only the pane in an uncovered repo is read");
});
