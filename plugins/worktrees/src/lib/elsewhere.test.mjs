import assert from "node:assert/strict";
import { test } from "node:test";

import { checkElsewhere, elsewhereRepos, findRepoFolders, folderOf, reposWithWorktrees } from "./elsewhere.ts";
import { parseWorktrees } from "./worktrees.ts";

const live = () => new AbortController().signal;

test("a scan looks for .git on each disk in turn and keeps the folder of each once, sorted", async () => {
  const signal = live();
  const calls = [];
  globalThis.wings = {
    disks: async () => [{ mount: "/" }, { mount: "/Volumes/Backup" }],
    diskFind: async (path, options) => {
      calls.push({ path, names: options.names, skip: options.skip, signal: options.signal });
      options.onProgress({ entries: path === "/" ? 50 : 30, bytes: 0, path });
      return path === "/" ? { paths: ["/a/.git", "/a/wt/.git", "/b/.git"], entries: 100 } : { paths: ["/Volumes/Backup/c/.git", "/a/.git"], entries: 40 };
    },
  };
  const looked = [];
  const folders = await findRepoFolders(signal, (n) => looked.push(n));
  assert.deepEqual(
    calls.map((c) => [c.path, c.names, c.skip, c.signal === signal]),
    [
      ["/", [".git"], ["node_modules", ".Trash"], true],
      ["/Volumes/Backup", [".git"], ["node_modules", ".Trash"], true],
    ],
  );
  assert.deepEqual(folders, ["/Volumes/Backup/c", "/a", "/a/wt", "/b"]);
  // What the second disk looked at adds to what the first did.
  assert.deepEqual(looked, [50, 130, 140]);
  assert.equal(folderOf("/.git"), "/");
});

const block = (path, extra = "") => `worktree ${path}\nHEAD ${"1".repeat(40)}\nbranch refs/heads/b\n${extra}`;
const ok = (stdout) => ({ code: 0, stdout, stderr: "" });

test("only repos with worktrees to manage are kept, once each however many of their folders were found", async () => {
  const answers = {
    "/r": ok(`${block("/r")}\n${block("/r-x")}`),
    // Found as a worktree: git says the same of it.
    "/r-x": ok(`${block("/r")}\n${block("/r-x")}`),
    "/solo": ok(block("/solo")),
    "/gone": ok(`${block("/gone")}\n${block("/gone-old", "prunable gitdir file points to non-existent location\n")}`),
    "/broken": { code: 128, stdout: "", stderr: "fatal: detected dubious ownership in repository" },
  };
  globalThis.wings = { exec: async (_program, _args, { cwd }) => answers[cwd] };
  assert.deepEqual(await reposWithWorktrees(["/r", "/r-x", "/solo", "/gone", "/broken"], live()), ["/gone", "/r"]);

  const stopped = new AbortController();
  stopped.abort();
  await assert.rejects(reposWithWorktrees(["/r"], stopped.signal), { message: "The scan was stopped" });
});

test("what is stored counts only when it has the shape of a scan", () => {
  assert.deepEqual(checkElsewhere({ at: 5, folders: ["/a", 3, "/b"] }), { at: 5, folders: ["/a", "/b"] });
  for (const junk of [null, 4, "x", {}, { at: "5", folders: [] }, { at: 5 }, { at: NaN, folders: [] }, { at: 5, folders: "/a" }]) assert.equal(checkElsewhere(junk), undefined, JSON.stringify(junk));
});

test("a repo is listed as elsewhere when it has worktrees and isn't one of the repos shown above", () => {
  const repo = (root, ...more) => ({ root, list: parseWorktrees([root, ...more].map((p) => `worktree ${p}\nHEAD 1\n`).join("\n")) });
  const found = [repo("/a", "/a-x"), repo("/b"), repo("/c", "/c-x"), repo("/d", "/d-x")];
  assert.deepEqual(
    elsewhereRepos(found, [repo("/c"), repo("/e")]).map((r) => r.root),
    ["/a", "/d"],
  );
});
