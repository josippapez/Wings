import test from "node:test";
import assert from "node:assert/strict";
import { applyOp, migrateTasks } from "./issues.ts";
import { branchOps, commitOps, keysIn, parseLog, scanProject } from "./autolink.ts";

const store = (n = 3) => {
  let s = migrateTasks([], "web");
  for (let i = 1; i <= n; i++) s = applyOp(s, { kind: "create", title: `t${i}` }, "you", 1).store;
  return s;
};
const commit = (sha, message) => ({ sha, subject: message.split("\n")[0], message });
const log = (...cs) => cs.map((c) => `${c.sha}\x1f${c.message}\n`).join("\0");
const get = (s, key) => s.issues.find((i) => i.key === key);

test("keysIn: word boundary, any case, several keys in one message, each once", () => {
  assert.deepEqual(keysIn("Fix WIN-12 and win-3, again WIN-12", "WIN"), ["WIN-12", "WIN-3"]);
  assert.deepEqual(keysIn("feature/WIN-12-login", "WIN"), ["WIN-12"]);
  assert.deepEqual(keysIn("(WIN-1)", "WIN"), ["WIN-1"]);
  assert.deepEqual(keysIn("AWIN-12 WIN-12a WIN-12_x WINX-1 WIN-", "WIN"), []);
  assert.deepEqual(keysIn("WIN-123", "WIN").includes("WIN-12"), false);
});

test("parseLog reads NUL-separated records with the whole message", () => {
  const out = log(commit("a".repeat(40), "WEB-1 one\n\nbody WEB-2"), commit("b".repeat(40), "two"));
  const cs = parseLog(out);
  assert.equal(cs.length, 2);
  assert.equal(cs[0].subject, "WEB-1 one");
  assert.match(cs[0].message, /body WEB-2/);
  assert.deepEqual(parseLog(""), []);
});

test("commitOps: only existing keys, a commit naming two issues goes to both, oldest first", () => {
  const s = store(2);
  const ops = commitOps(s, [commit("c".repeat(40), "web-2 later"), commit("d".repeat(40), "WEB-1, WEB-2 and WEB-9\n\nmore"), commit("e".repeat(40), "WEB-1 first")]);
  const by = Object.fromEntries(ops.map((o) => [o.key, o.commits.map((c) => c.sha[0])]));
  assert.deepEqual(by, { "WEB-1": ["e", "d"], "WEB-2": ["d", "c"] });
});

test("an autolink op adds each commit once with a log entry by git, and a second run adds nothing", () => {
  let s = store(1);
  const op = { kind: "autolink", key: "WEB-1", commits: [{ sha: "a".repeat(40), subject: "WEB-1 first" }, { sha: "b1b2b3b4b5", subject: "WEB-1 second" }, { sha: "a".repeat(40), subject: "dup" }] };
  const first = applyOp(s, op, "git", 5);
  assert.equal(first.changed, true);
  assert.deepEqual(get(first.store, "WEB-1").links.commits, ["aaaaaaa", "b1b2b3b"]);
  const entries = get(first.store, "WEB-1").log.filter((e) => e.by === "git");
  assert.deepEqual(entries.map((e) => e.text), ["Commit aaaaaaa: WEB-1 first", "Commit b1b2b3b: WEB-1 second"]);
  assert.equal(applyOp(first.store, op, "git", 6).changed, false);
  // a commit an agent added by hand is not added again
  const manual = applyOp(s, { kind: "update", key: "WEB-1", commit: "aaaaaaa" }, { id: "s" }, 3).store;
  assert.deepEqual(get(applyOp(manual, op, "git", 6).store, "WEB-1").links.commits, ["aaaaaaa", "b1b2b3b"]);
  assert.equal(get(applyOp(first.store, { kind: "autolink", key: "WEB-1", commits: [{ sha: "zzz", subject: "x" }] }, "git").store, "WEB-1").links.commits.length, 2);
});

test("an autolink by git doesn't count as an agent session", () => {
  const s = applyOp(store(1), { kind: "autolink", key: "WEB-1", commits: [{ sha: "a".repeat(40), subject: "x" }] }, "git", 5).store;
  assert.deepEqual(get(s, "WEB-1").sessions, []);
});

test("branchOps: the first existing key in a branch name sets an empty branch, once", () => {
  let s = store(3);
  assert.deepEqual(branchOps(s, ["feature/web-2-login", "feature/web-2-login", "main", "fix/WEB-9-x"]).map((o) => [o.key, o.branch]), [["WEB-2", "feature/web-2-login"]]);
  assert.deepEqual(branchOps(s, ["WEB-1-and-WEB-3"]).map((o) => o.key), ["WEB-1"]);
  const set = applyOp(s, branchOps(s, ["feature/WEB-2-x"])[0], "git", 5);
  assert.equal(get(set.store, "WEB-2").links.branch, "feature/WEB-2-x");
  assert.deepEqual(get(set.store, "WEB-2").log.at(-1), { at: 5, by: "git", kind: "link", text: "Branch: feature/WEB-2-x" });
  assert.deepEqual(branchOps(set.store, ["other/WEB-2-y"]), []);
  const manual = applyOp(s, { kind: "update", key: "WEB-2", branch: "mine" }, "you").store;
  assert.equal(applyOp(manual, { kind: "autolink", key: "WEB-2", branch: "feature/WEB-2-x" }, "git").changed, false);
});

test("scanProject: one git log per project and one branch call per other pane folder, none without issues", async () => {
  const calls = [];
  const exec = async (program, args, { cwd }) => {
    calls.push([program, args[0], cwd]);
    if (args[0] === "log") return { code: 0, stdout: log(commit("f".repeat(40), "WEB-1 done")) };
    return { code: 0, stdout: cwd.endsWith("wt-2") ? "feature/WEB-2-x\n" : "main\n" };
  };
  const project = { path: "/work/web", branch: "feature/WEB-1-y" };
  const r = await scanProject(exec, project, ["/work/web", "/work/web/", "/work/wt-2", "/work/other"], store(2), 0);
  assert.equal(r.calls, 3);
  assert.equal(calls.filter((c) => c[1] === "log").length, 1);
  assert.deepEqual(calls.filter((c) => c[1] === "log").map((c) => c[2]), ["/work/web"]);
  assert.deepEqual(r.ops.map((o) => [o.key, o.commits?.length ?? o.branch]), [["WEB-1", 1], ["WEB-1", "feature/WEB-1-y"], ["WEB-2", "feature/WEB-2-x"]]);
  calls.length = 0;
  assert.deepEqual(await scanProject(exec, project, ["/x"], store(0), 0), { ops: [], calls: 0 });
  assert.equal(calls.length, 0);
  const failing = await scanProject(async () => ({ code: 128, stdout: "" }), project, [], store(1), 0);
  assert.deepEqual(failing.ops.map((o) => o.commits), [undefined], "a failed git log links no commits; the branch Wings reports still counts");
});
