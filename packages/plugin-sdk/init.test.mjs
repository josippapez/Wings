// node --test: init makes a plugin from the template, named after its folder, that builds.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { buildPlugin } from "./build.mjs";
import { idOf, init } from "./init.mjs";

const sdk = JSON.parse(readFileSync(join(import.meta.dirname, "package.json"), "utf8"));
const json = (dir, file) => JSON.parse(readFileSync(join(dir, file), "utf8"));

function scratch(t) {
  const root = mkdtempSync(join(tmpdir(), "wings-init-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test("init names the plugin after its folder, pins this SDK and adds the scripts, and the result builds", async (t) => {
  const dir = join(scratch(t), "My Cool_Plugin");
  const lines = [];
  init(dir, { install: false, log: (l) => lines.push(l) });

  const manifest = json(dir, "wings-plugin.json");
  assert.equal(manifest.id, "my-cool-plugin");
  assert.equal(manifest.name, "My cool plugin");
  const pkg = json(dir, "package.json");
  assert.equal(pkg.devDependencies["wings-plugin-sdk"], `^${sdk.version}`);
  assert.deepEqual(Object.keys(pkg.scripts).sort(), ["build", "check", "dev", "pack"]);
  // The template keeps its ignore file as `gitignore`, which npm would otherwise leave out.
  assert.ok(existsSync(join(dir, ".gitignore")) && !existsSync(join(dir, "gitignore")));
  assert.ok(existsSync(join(dir, ".github", "workflows", "release.yml")));
  assert.ok(!readFileSync(join(dir, "README.md"), "utf8").includes("hello-wings"));

  // Install the packages the way npm would: this SDK, its React, and a stand-in for wings-plugin-ui.
  const modules = join(dir, "node_modules");
  mkdirSync(modules);
  symlinkSync(import.meta.dirname, join(modules, "wings-plugin-sdk"));
  for (const name of ["react", "react-dom"]) symlinkSync(join(import.meta.dirname, "node_modules", name), join(modules, name));
  const ui = join(modules, "wings-plugin-ui");
  mkdirSync(ui);
  writeFileSync(join(ui, "package.json"), JSON.stringify({ name: "wings-plugin-ui", type: "module", exports: "./index.js" }));
  writeFileSync(join(ui, "index.js"), ["Empty", "List", "ListItem", "Loading", "Stack", "Text", "showSidebar", "showTab", "showView", "showBlock", "showPopover", "showSettings"].map((n) => `export const ${n} = () => {};`).join("\n"));

  const { built } = await buildPlugin(dir);
  assert.ok(built.some((b) => b.file.endsWith(join("dist", "main.js"))));
  assert.ok(existsSync(join(dir, "dist", "sidebar.html")));
});

test("init runs the package manager it is given in the new folder", (t) => {
  const dir = join(scratch(t), "app");
  const calls = [];
  init(dir, { install: "pnpm", run: (command, args, cwd) => (calls.push([command, args, cwd]), 0), log: () => {} });
  assert.deepEqual(calls, [["pnpm", ["install"], dir]]);
  assert.throws(() => init(join(scratch(t), "bad"), { install: "pnpm", run: () => 1, log: () => {} }), /pnpm install failed/);
});

test("init refuses a folder that has files, and a name that makes no id", (t) => {
  const root = scratch(t);
  const full = join(root, "full");
  mkdirSync(full);
  writeFileSync(join(full, "notes.txt"), "mine");
  assert.throws(() => init(full, { install: false, log: () => {} }), /already has files/);
  assert.equal(readFileSync(join(full, "notes.txt"), "utf8"), "mine");
  assert.throws(() => init(join(root, "___"), { install: false, log: () => {} }), /Can't make a plugin id/);
  // An empty folder is fine.
  const empty = join(root, "empty");
  mkdirSync(empty);
  assert.equal(init(empty, { install: false, log: () => {} }).id, "empty");
});

test("an id has only lowercase letters, digits and dashes, up to 64", () => {
  assert.equal(idOf("/x/Hello, World!"), "hello-world");
  assert.equal(idOf("/x/-a--b-"), "a-b");
  assert.equal(idOf(`/x/${"a".repeat(70)}`).length, 64);
});
