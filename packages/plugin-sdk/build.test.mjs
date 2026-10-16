// node --test: the build's entries, dist/ output, generated pages and mounts, CSS, and pack.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { unzipSync } from "fflate";

import { buildPlugin, pack } from "./build.mjs";

const boxBlocks = [{ id: "box", title: "Box", page: "dist/box.html" }];
const tabView = { id: "item", title: "Item", icon: "list", page: "dist/sidebar.html", opens: ["tab"] };
const ping = { name: "ping", description: "Pings.", inputSchema: { type: "object" } };

/** A plugin folder with this package installed, like `npm link wings-plugin-sdk` makes it. A file set to null is left out, and `patch` changes the manifest. */
function plugin(files, patch = {}) {
  const dir = mkdtempSync(join(tmpdir(), "wings-plugin-test-"));
  mkdirSync(join(dir, "node_modules"));
  symlinkSync(import.meta.dirname, join(dir, "node_modules", "wings-plugin-sdk"));
  const manifest = {
    id: "demo",
    name: "Demo",
    version: "0.1.0",
    api: 1,
    main: "dist/main.js",
    contributes: { sidebars: [{ id: "demo", title: "Demo <1>", icon: "list", page: "dist/sidebar.html" }] },
  };
  files = { "wings-plugin.json": JSON.stringify({ ...manifest, ...patch, contributes: { ...manifest.contributes, ...patch.contributes } }), "src/main.ts": "wings.onPanes(() => {});", "src/pages/sidebar.ts": 'import { mountShell } from "wings-plugin-sdk/shell";\nimport "./sidebar.css";\nmountShell("sidebar").textContent = "hi";', "src/pages/sidebar.css": ".mine { color: red; }", ...files };
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
  }, { contributes: { blocks: boxBlocks } });
  t.after(() => rmSync(dir, { recursive: true }));
  // A wings-plugin-ui that records what a page shows, and the React this package was installed with.
  const ui = join(dir, "node_modules", "wings-plugin-ui");
  mkdirSync(ui);
  writeFileSync(join(ui, "package.json"), JSON.stringify({ name: "wings-plugin-ui", type: "module", exports: "./index.js" }));
  const show = (place) => `export const show${place} = (element, options) => { globalThis.shown = { place: "${place}", element, options }; };`;
  writeFileSync(join(ui, "index.js"), ["Sidebar", "Tab", "View", "Block", "Popover", "Settings"].map(show).join("\n"));
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
  // A block fits its height to the page.
  const box = shown("box");
  assert.equal(box.place, "Block");
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
  const dir = plugin({ "src/pages/box.ts": 'import { mountShell } from "wings-plugin-sdk/shell";\nmountShell("view");' }, { contributes: { blocks: boxBlocks } });
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

test("a plugin with src/tools.ts gets its manifest's tools written by build, checked by check, and packed without touching the folder", async (t) => {
  const tools = 'import { s, tool } from "wings-plugin-sdk/tools";\nexport const tools = { ping: tool({ description: "Pings.", input: s.object({ n: s.integer({ minimum: 1 }) }), run: () => "pong" }) };';
  const dir = plugin({ "src/tools.ts": tools, "src/main.ts": 'import { registerTools } from "wings-plugin-sdk/tools";\nimport { tools } from "./tools";\nregisterTools(tools);' });
  t.after(() => rmSync(dir, { recursive: true }));
  const manifest = () => JSON.parse(readFileSync(join(dir, "wings-plugin.json"), "utf8"));
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale.map((f) => f.slice(dir.length + 1)).filter((f) => !f.startsWith("dist/")), ["wings-plugin.json"]);
  const before = readFileSync(join(dir, "wings-plugin.json"), "utf8");
  const zip = unzipSync(readFileSync(await pack(dir)));
  assert.equal(JSON.parse(new TextDecoder().decode(zip["wings-plugin.json"])).contributes.mcpTools[0].name, "ping");
  assert.equal(readFileSync(join(dir, "wings-plugin.json"), "utf8"), before);
  await buildPlugin(dir);
  assert.deepEqual(manifest().contributes.mcpTools, [{ name: "ping", description: "Pings.", inputSchema: { type: "object", properties: { n: { type: "integer", minimum: 1 } }, required: ["n"], additionalProperties: false } }]);
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale, []);
});

test("a manifest that disagrees with the code fails build, check and pack, naming the file and the field", async (t) => {
  const sidebar = { id: "demo", title: "Demo", icon: "list", page: "dist/sidbar.html" };
  const cases = [
    [{ main: "mian.js" }, {}, /wings-plugin\.json: main is "mian\.js", but the build writes the main script to dist\/main\.js/],
    [{ contributes: { sidebars: [sidebar] } }, {}, /contributes\.sidebars\[0\]\.page is "dist\/sidbar\.html".* It writes dist\/sidebar\.html/],
    [{ icon: "icn.svg" }, { "icon.svg": "<svg/>" }, /wings-plugin\.json: icon is "icn\.svg", which the plugin folder doesn't have/],
    [{ contributes: { mcpTools: [ping] } }, { "src/main.ts": 'wings.onTool("pong", () => "pong");' }, /dist\/main\.js answers "pong".*\(it has ping\).*contributes\.mcpTools lists "ping"/s],
    [{ contributes: { hooks: ["SessionStart"] } }, { "src/main.ts": 'wings.onHook("PreToolUse", () => null);' }, /dist\/main\.js answers "PreToolUse" with wings\.onHook, but contributes\.hooks doesn't list it \(it lists SessionStart\).*contributes\.hooks lists "SessionStart", but nothing/s],
    [{}, { "src/main.ts": 'wings.setBadge("p", null);' }, /dist\/main\.js calls wings\.setBadge, so contributes\.ui must list "badges"/],
    [{}, { "src/main.ts": 'wings.openView("sidbar");' }, /dist\/main\.js calls wings\.openView\("sidbar"\).*\(they have demo\)/],
    [{}, { "src/main.ts": 'wings.closeView("sidbar");\nwings.setSidebarLabel("sidbar", null);' }, /wings\.closeView\("sidbar"\).*wings\.setSidebarLabel\("sidbar"\)/s],
    [{ contributes: { ui: ["status"] } }, { "src/main.ts": 'wings.setPaneStatus("p", { items: [], open: "sidbar" });' }, /dist\/main\.js sets open: "sidbar" on a pane status or rail, but contributes\.sidebars and contributes\.views have no such id \(they have demo\)/],
    [{ contributes: { views: [tabView] } }, { "src/main.ts": 'wings.setProjectSection("p", "s", [{ id: "a", label: "A", open: { view: "itm" } }]);' }, /dist\/main\.js sets open: \{ view: "itm" \} on a project section row or a notification button, but contributes\.views has no view of that id with "tab" in opens \(the views that have it: item\)/],
    // A sidebar is a page, but a button can't open it as a tab.
    [{ contributes: { views: [tabView] } }, { "src/main.ts": 'wings.notify({ title: "t", actions: [{ label: "Open", open: { view: "demo", args: 1 } }] });' }, /sets open: \{ view: "demo" \}.*\(the views that have it: item\)/],
  ];
  for (const [patch, files, message] of cases) {
    const dir = plugin(files, patch);
    t.after(() => rmSync(dir, { recursive: true }));
    await assert.rejects(buildPlugin(dir), message);
    await assert.rejects(buildPlugin(dir, { check: true }), message);
    await assert.rejects(pack(dir), message);
  }
});

test("a manifest that agrees with the code builds, and a name the code computes is left alone", async (t) => {
  const main = [
    "const id = String(Date.now());",
    'wings.onTool("ping", () => "pong");',
    'wings.onTool(id, () => "x");',
    'wings.setBadge("p", null);',
    'wings.openView("demo");',
    "wings.openView(id);",
    // A string joined to something else isn't a name.
    'wings.openView("x-" + id);',
    'wings.closeView("demo");',
    'wings.setSidebarLabel("item", null);',
    'wings.setPaneStatus("p", { items: [], open: "demo" });',
    'wings.setPaneStatus("p", { items: [], open: id });',
    'wings.setProjectSection("p", "s", [{ id: "a", label: "A", open: { view: "item", args: id } }]);',
    'wings.notify({ title: "t", actions: [{ label: "Open", open: { view: "item" } }, { label: "Other", open: { view: `item-${id}` } }] });',
  ].join("\n");
  const dir = plugin({ "src/main.ts": main, "icon.svg": "<svg/>" }, { icon: "icon.svg", contributes: { ui: ["badges", "status"], views: [tabView], mcpTools: [ping, { ...ping, name: "other" }] } });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  assert.deepEqual((await buildPlugin(dir, { check: true })).stale, []);
  await pack(dir);
});

test("a bundle leaves out the SDK helpers it doesn't import", async (t) => {
  const dir = plugin({ "src/main.ts": 'import { gitOk } from "wings-plugin-sdk/git";\nwings.onPanes(() => gitOk("/", ["status"]));' });
  t.after(() => rmSync(dir, { recursive: true }));
  await buildPlugin(dir);
  const main = readFileSync(join(dir, "dist", "main.js"), "utf8");
  assert.ok(main.includes("status"));
  // repoRoot and defaultBranch live in the same module as gitOk.
  assert.ok(!main.includes("--show-toplevel") && !main.includes("origin/HEAD"));
});
