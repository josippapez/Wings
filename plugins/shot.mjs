// `pnpm shot [filter...] [--width 1000,640] [--height 900] [--out <dir>]`: screenshots plugin pages in headless Chrome, so a
// page is looked at before it ships. Every plugins/<plugin>/preview/<name>.tsx is a fixture: a module that renders a page
// component with made-up data. Chrome comes from CHROME=/path/to/chrome, else the usual macOS and Linux locations.
//
// A fixture's exports:
//   default   a React element, or a component, to show
//   fill      true for a page that fills the tab (`showTab(..., { fill: true })`): the frame is as tall as --height.
//             Without it the PNG is as tall as the page.
//   place     "tab" (default), "view", "settings", "sidebar" or "popover": which `show*` mounts it
//   project   what `wings.project()` answers (default: a project "web"; null for none)
//   args      what `wings.viewArgs()` answers (default: null)
//   respond   (message) => answer | undefined: what main answers a broadcast with, delivered to `onBroadcast`
// The page gets plugin-ui.css from packages/plugin-ui and the CSS its imports bring, in the app's dark theme, with a
// stub `window.wings` (shot-stub.js). Exits 1 when a fixture throws or logs a console error, 2 without Chrome.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, parse } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const root = import.meta.dirname;

const argv = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = argv.indexOf(name);
  return i < 0 ? fallback : argv.splice(i, 2)[1];
};
const widths = flag("--width", "1000,640").split(",").map(Number);
const height = Number(flag("--height", "900"));
const out = flag("--out", null) ?? mkdtempSync(join(tmpdir(), "wings-shot-"));
const filters = argv;
if (widths.some((w) => !(w > 0)) || !(height > 0)) {
  console.error("--width is a comma-separated list of pixel widths, and --height a number.");
  process.exit(2);
}

const chrome = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((p) => p && existsSync(p));
if (!chrome) {
  console.error("shot needs Chrome: install it or set CHROME=/path/to/chrome.");
  process.exit(2);
}

// The fixtures: plugins/<plugin>/preview/<name>.tsx (a file starting with _ is shared data, not one), picked by a filter that is a substring of "<plugin>/<name>".
const fixtures = readdirSync(root, { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(root, d.name, "preview")))
  .flatMap((d) => readdirSync(join(root, d.name, "preview")).filter((f) => /^[^_].*\.(tsx?|jsx?)$/.test(f)).map((f) => ({ id: `${d.name}/${parse(f).name}`, file: join(root, d.name, "preview", f) })))
  .filter((f) => !filters.length || filters.some((s) => f.id.includes(s)));
if (!fixtures.length) {
  console.error(`No fixture matches ${filters.join(" ") || "anything"}. They are plugins/<plugin>/preview/<name>.tsx.`);
  process.exit(2);
}
mkdirSync(out, { recursive: true });

// esbuild is the plugin SDK's dependency, so it is found from there.
const esbuild = await import(createRequire(realpathSync(join(root, "node_modules/wings-plugin-sdk/build.mjs"))).resolve("esbuild"));
const uiCss = readFileSync(join(root, "../packages/plugin-ui/plugin-ui.css"), "utf8");
const react = (p) => realpathSync(join(root, "node_modules", p));
const work = mkdtempSync(join(tmpdir(), "wings-shot-work-"));

/** Bundles one fixture, as a page: the fixture, the stub and wings-plugin-ui's `show*`, mounted the way a plugin's own entry does. */
async function pageOf(fixture) {
  const result = await esbuild.build({
    stdin: {
      resolveDir: join(fixture.file, ".."),
      loader: "js",
      contents: `
        import { installWings } from ${JSON.stringify(join(root, "shot-stub.js"))};
        import * as fx from ${JSON.stringify(fixture.file)};
        import { createElement } from "react";
        import { showTab, showView, showSidebar, showPopover, showSettings } from "wings-plugin-ui";
        installWings(fx);
        const show = { tab: showTab, view: showView, sidebar: showSidebar, popover: showPopover, settings: showSettings }[fx.place ?? "tab"];
        if (!show) throw new Error("place is tab, view, settings, sidebar or popover, not " + fx.place);
        show(typeof fx.default === "function" ? createElement(fx.default) : fx.default, { fill: Boolean(fx.fill) });
      `,
    },
    outdir: join(work, fixture.id.replace("/", "-")),
    bundle: true,
    format: "iife",
    jsx: "automatic",
    // React's development build, so its warnings reach the console.
    define: { "process.env.NODE_ENV": '"development"' },
    alias: { react: react("react"), "react-dom": react("react-dom") },
    write: false,
    logLevel: "silent",
  });
  const text = (ext) => result.outputFiles.find((f) => f.path.endsWith(ext))?.text ?? "";
  const dir = join(work, fixture.id.replace("/", "-"));
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "page.js"), text(".js"));
  writeFileSync(join(dir, "page.html"), `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><style>${uiCss}</style><style>${text(".css")}</style></head><body><main id="app"></main><script src="page.js"></script></body></html>\n`);
  return pathToFileURL(join(dir, "page.html")).href;
}

// ---------- Chrome over the DevTools protocol ----------

const profile = mkdtempSync(join(tmpdir(), "wings-shot-chrome-"));
const proc = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
const cleanup = () => {
  proc.kill();
  // Chrome is still writing to its profile as it dies.
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  rmSync(work, { recursive: true, force: true });
};
process.on("exit", cleanup);
const wsUrl = await new Promise((resolve, reject) => {
  let log = "";
  proc.stderr.on("data", (d) => {
    log += d;
    const m = log.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) resolve(m[1]);
  });
  proc.on("exit", () => reject(new Error(`Chrome exited: ${log}`)));
});
const ws = new WebSocket(wsUrl);
await new Promise((r) => (ws.onopen = r));
let nextId = 0;
const waiting = new Map();
const events = [];
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.id && waiting.has(m.id)) {
    const { resolve, reject } = waiting.get(m.id);
    waiting.delete(m.id);
    m.error ? reject(new Error(m.error.message)) : resolve(m.result);
  } else if (m.method) events.push(m);
};
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    waiting.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Opens the page, waits for it to draw, and shoots it at each width. Resolves the console errors it logged. */
async function shoot(fixture, url) {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  const errors = [];
  const text = (args) => args.map((a) => a.value ?? a.description ?? a.type).join(" ");
  const drain = () => {
    for (const m of events.splice(0)) {
      if (m.sessionId !== sessionId) continue;
      if (m.method === "Runtime.exceptionThrown") errors.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
      if (m.method === "Runtime.consoleAPICalled" && (m.params.type === "error" || m.params.type === "assert")) errors.push(text(m.params.args));
      if (m.method === "Log.entryAdded" && m.params.entry.level === "error") errors.push(`${m.params.entry.text} ${m.params.entry.url ?? ""}`.trim());
    }
  };
  await call("Page.enable");
  await call("Runtime.enable");
  await call("Log.enable");
  await call("Emulation.setDeviceMetricsOverride", { width: widths[0], height, deviceScaleFactor: 1, mobile: false });
  await call("Page.navigate", { url });
  const drawn = async () => (await call("Runtime.evaluate", { expression: "document.getElementById('app')?.children.length > 0", returnByValue: true })).result.value === true;
  for (let i = 0; i < 100 && !(await drawn()); i++) await sleep(100);
  await call("Runtime.evaluate", { expression: "document.fonts.ready", awaitPromise: true });
  await sleep(600);
  const files = [];
  for (const width of widths) {
    await call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    await sleep(300);
    // A page that fills the tab is the frame's height; any other is as tall as its content.
    const full = (await call("Runtime.evaluate", { expression: "document.documentElement.scrollHeight", returnByValue: true })).result.value;
    const shot = { format: "png" };
    if (full > height) Object.assign(shot, { captureBeyondViewport: true, clip: { x: 0, y: 0, width, height: full, scale: 1 } });
    const { data } = await call("Page.captureScreenshot", shot);
    const file = join(out, `${fixture.id.replace("/", "-")}-${width}.png`);
    writeFileSync(file, Buffer.from(data, "base64"));
    files.push(file);
  }
  drain();
  await send("Target.closeTarget", { targetId });
  return { files, errors };
}

console.error(`Screenshots go to ${out}`);
let failed = 0;
for (const fixture of fixtures) {
  try {
    const { files, errors } = await shoot(fixture, await pageOf(fixture));
    for (const f of files) console.log(f);
    if (errors.length) {
      failed++;
      console.error(`${fixture.id} logged ${errors.length} console error(s):`);
      for (const e of errors) console.error(`  ${e.split("\n").slice(0, 4).join("\n  ")}`);
    }
  } catch (e) {
    failed++;
    console.error(`${fixture.id} failed: ${e.message ?? e}`);
  }
}
process.exit(failed ? 1 : 0);
