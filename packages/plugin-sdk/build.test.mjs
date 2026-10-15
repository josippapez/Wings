// node --test: the build's entries, dist/ output, generated pages and mounts, CSS, and pack.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { unzipSync } from "fflate";

import { buildPlugin, pack } from "./build.mjs";

/** A plugin folder with this package installed, like `npm link wings-plugin-sdk` makes it. A file set to null is left out. */
function plugin(files) {
  const dir = mkdtempSync(join(tmpdir(), "wings-plugin-test-"));
  mkdirSync(join(dir, "node_modules"));
  symlinkSync(import.meta.dirname, join(dir, "node_modules", "wings-plugin-sdk"));
  const manifest = {
    id: "demo",
    name: "Demo",
    version: "0.1.0",
    api: 1,
    main: "dist/main.js",
    contributes: { sidebars: [{ id: "demo", title: "Demo <1>", icon: "list", page: "dist/sidebar.html" }], blocks: [{ id: "box", title: "Box", page: "dist/box.html" }] },
  };
  files = { "wings-plugin.json": JSON.stringify(manifest), "src/main.ts": "wings.onPanes(() => {});", "src/pages/sidebar.ts": 'import { mountShell } from "wings-plugin-sdk/shell";\nimport "./sidebar.css";\nmountShell("sidebar").textContent = "hi";', "src/pages/sidebar.css": ".mine { color: red; }", ...files };
  for (const [name, text] of Object.entries(files)) {
    if (text === null) continue;
    mkdirSync(join(dir, name, ".."), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  return dir;
}

test("a page that mounts with the shell gets its HTML and its CSS, all in dist/", async (t) => {
  const dir = plugin({});
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  const html = readFileSync(join(dir, "dist", "sidebar.html"), "utf8");
  assert.match(html, /<title>Demo &lt;1&gt;<\/title>/);
  assert.match(html, /<link rel="stylesheet" href="sidebar.css" \/>/);
  assert.match(html, /<main id="app"><\/main>\s*<script src="sidebar.js"><\/script>/);
  assert.match(readFileSync(join(dir, "dist", "sidebar.css"), "utf8"), /\.mine\{color:red\}/);
  // main.ts doesn't mount a page, so it gets no HTML.
  assert.equal(existsSync(join(dir, "dist", "main.html")), false);
  assert.equal(existsSync(join(dir, "main.js")), false);
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale, []);
});

test("only src/main.ts and the files in src/pages/ are entries", async (t) => {
  const dir = plugin({ "src/helper.ts": "export const x = 1;", "src/lib/util.ts": "export const y = 2;" });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.deepEqual(readdirSync(join(dir, "dist")).sort(), ["main.js", "sidebar.css", "sidebar.html", "sidebar.js"]);
});

test("a React page is mounted from its default export in the place its page export names", async (t) => {
  const dir = plugin({
    "src/pages/sidebar.ts": null,
    "src/pages/sidebar.css": null,
    "src/pages/sidebar.tsx": 'export const page = { place: "sidebar" };\nexport default function Sidebar() { return <p>hi</p>; }',
    "src/pages/box.tsx": 'export const page = { place: "block", fill: true };\nexport default function Box() { return <p>box</p>; }',
  });
  t.after(() => rmSync(dir, { recursive: true }));
  // A wings-plugin-ui that records what a page shows, and the React this package was installed with.
  const ui = join(dir, "node_modules", "wings-plugin-ui");
  mkdirSync(ui);
  writeFileSync(join(ui, "package.json"), JSON.stringify({ name: "wings-plugin-ui", type: "module", exports: "./index.js" }));
  const show = (place) => `export const show${place} = (element, options) => { globalThis.shown = { place: "${place}", element, options }; };`;
  writeFileSync(join(ui, "index.js"), ["Sidebar", "Tab", "View", "Popover", "Settings"].map(show).join("\n"));
  symlinkSync(join(import.meta.dirname, "node_modules", "react"), join(dir, "node_modules", "react"));

  await buildPlugin(dir);
  const shown = (page) => {
    const context = {};
    runInNewContext(readFileSync(join(dir, "dist", `${page}.js`), "utf8"), context);
    return context.shown;
  };
  const sidebar = shown("sidebar");
  assert.equal(sidebar.place, "Sidebar");
  assert.equal(sidebar.element.type().props.children, "hi");
  assert.equal(sidebar.options.fill, undefined);
  // A block is a view for now.
  const box = shown("box");
  assert.equal(box.place, "View");
  assert.equal(box.element.type().props.children, "box");
  assert.equal(box.options.fill, true);
  assert.match(readFileSync(join(dir, "dist", "box.html"), "utf8"), /<title>Box<\/title>/);
});

test("a plugin without src/main.ts gets an empty dist/main.js", async (t) => {
  const dir = plugin({ "src/main.ts": null });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.match(readFileSync(join(dir, "dist", "main.js"), "utf8"), /^\/\/ Built from src\/[^\n]*\n$/);
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale, []);
});

test("a block page is titled by its manifest entry", async (t) => {
  const dir = plugin({ "src/pages/box.ts": 'import { mountShell } from "wings-plugin-sdk/shell";\nmountShell("view");' });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.match(readFileSync(join(dir, "dist", "box.html"), "utf8"), /<title>Box<\/title>/);
});

test("a hand-written page wins, and check notices a generated page that's out of date", async (t) => {
  const dir = plugin({ "dist/sidebar.html": "<main id=app></main><script src=sidebar.js></script>" });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.equal(readFileSync(join(dir, "dist", "sidebar.html"), "utf8"), "<main id=app></main><script src=sidebar.js></script>");

  const generated = plugin({});
  t.after(() => rmSync(generated, { recursive: true }));
  await buildPlugin(generated);
  writeFileSync(join(generated, "dist", "sidebar.html"), readFileSync(join(generated, "dist", "sidebar.html"), "utf8").replace("Demo", "Old"));
  assert.deepEqual((await buildPlugin(generated, { check: true })).stale, [join(generated, "dist", "sidebar.html")]);
});

test("a hand-written CSS file is never overwritten by the CSS built from src/", async (t) => {
  const dir = plugin({ "dist/sidebar.css": ".theirs {}" });
  t.after(() => rmSync(dir, { recursive: true }));
  await assert.rejects(buildPlugin(dir), /sidebar.css would be overwritten/);
  assert.equal(readFileSync(join(dir, "dist", "sidebar.css"), "utf8"), ".theirs {}");
});

test("pack zips what the plugin ships with the build in dist/, the manifest at the top, and leaves the folder as it was", async (t) => {
  const dir = plugin({ "icon.png": "png", "assets/logo.svg": "<svg/>", "README.md": "x", "package.json": "{}", "tsconfig.json": "{}", ".env": "x", "old.wings-plugin": "x", "node_modules/x/index.js": "x", "dist/main.js": "stale" });
  t.after(() => rmSync(dir, { recursive: true }));
  const file = await pack(dir);
  assert.equal(file, join(dir, "demo.wings-plugin"));
  const zip = unzipSync(new Uint8Array(readFileSync(file)));
  assert.deepEqual(Object.keys(zip).sort(), ["README.md", "assets/logo.svg", "dist/main.js", "dist/sidebar.css", "dist/sidebar.html", "dist/sidebar.js", "icon.png", "wings-plugin.json"]);
  assert.match(new TextDecoder().decode(zip["dist/sidebar.html"]), /<script src="sidebar.js">/);
  // A committed dist/ doesn't leak into the package: the manifest's main is the fresh build.
  assert.match(new TextDecoder().decode(zip["dist/main.js"]), /wings\.onPanes/);
  assert.equal(readFileSync(join(dir, "dist", "main.js"), "utf8"), "stale");
  assert.equal(existsSync(join(dir, "dist", "sidebar.html")), false);
});
