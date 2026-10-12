// node --test: the build's generated pages and CSS, and pack.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { unzipSync } from "fflate";

import { buildPlugin, pack } from "./build.mjs";

/** A plugin folder with this package installed, like `npm link wings-plugin-sdk` makes it. */
function plugin(files) {
  const dir = mkdtempSync(join(tmpdir(), "wings-plugin-test-"));
  mkdirSync(join(dir, "node_modules"));
  symlinkSync(import.meta.dirname, join(dir, "node_modules", "wings-plugin-sdk"));
  const manifest = { id: "demo", name: "Demo", version: "0.1.0", api: 1, main: "main.js", contributes: { sidebars: [{ id: "demo", title: "Demo <1>", icon: "list", page: "sidebar.html" }] } };
  files = { "wings-plugin.json": JSON.stringify(manifest), "src/main.ts": "wings.onPanes(() => {});", "src/sidebar.ts": 'import { mountShell } from "wings-plugin-sdk/shell";\nimport "./sidebar.css";\nmountShell("sidebar").textContent = "hi";', "src/sidebar.css": ".mine { color: red; }", ...files };
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(join(dir, name, ".."), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  return dir;
}

test("a page that mounts with the shell gets its HTML and its CSS", async (t) => {
  const dir = plugin({});
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  const html = readFileSync(join(dir, "sidebar.html"), "utf8");
  assert.match(html, /<title>Demo &lt;1&gt;<\/title>/);
  assert.match(html, /<link rel="stylesheet" href="sidebar.css" \/>/);
  assert.match(html, /<main id="app"><\/main>\s*<script src="sidebar.js"><\/script>/);
  assert.match(readFileSync(join(dir, "sidebar.css"), "utf8"), /\.mine\{color:red\}/);
  // main.ts doesn't mount a page, so it gets no HTML.
  assert.equal(existsSync(join(dir, "main.html")), false);
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale, []);
});

test("a hand-written page wins, and check notices a generated page that's out of date", async (t) => {
  const dir = plugin({ "sidebar.html": "<main id=app></main><script src=sidebar.js></script>" });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.equal(readFileSync(join(dir, "sidebar.html"), "utf8"), "<main id=app></main><script src=sidebar.js></script>");

  const generated = plugin({});
  t.after(() => rmSync(generated, { recursive: true }));
  await buildPlugin(generated);
  writeFileSync(join(generated, "sidebar.html"), readFileSync(join(generated, "sidebar.html"), "utf8").replace("Demo", "Old"));
  assert.deepEqual((await buildPlugin(generated, { check: true })).stale, [join(generated, "sidebar.html")]);
});

test("a hand-written CSS file is never overwritten by the CSS built from src/", async (t) => {
  const dir = plugin({ "sidebar.css": ".theirs {}" });
  t.after(() => rmSync(dir, { recursive: true }));
  await assert.rejects(buildPlugin(dir), /sidebar.css would be overwritten/);
  assert.equal(readFileSync(join(dir, "sidebar.css"), "utf8"), ".theirs {}");
});

test("pack zips what the plugin ships, built, with the manifest at the top, and leaves the folder unbuilt", async (t) => {
  const dir = plugin({ "icon.png": "png", "assets/logo.svg": "<svg/>", "README.md": "x", "package.json": "{}", "tsconfig.json": "{}", ".env": "x", "old.wings-plugin": "x", "node_modules/x/index.js": "x" });
  t.after(() => rmSync(dir, { recursive: true }));
  const file = await pack(dir);
  assert.equal(file, join(dir, "demo.wings-plugin"));
  const zip = unzipSync(new Uint8Array(readFileSync(file)));
  assert.deepEqual(Object.keys(zip).sort(), ["assets/logo.svg", "icon.png", "main.js", "sidebar.css", "sidebar.html", "sidebar.js", "wings-plugin.json"]);
  assert.match(new TextDecoder().decode(zip["sidebar.html"]), /<script src="sidebar.js">/);
  assert.equal(existsSync(join(dir, "main.js")), false);
  assert.equal(existsSync(join(dir, "sidebar.html")), false);
});
