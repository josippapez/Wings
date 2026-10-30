// `wings-plugin shot [filter...] [--width 1000,640] [--height 900] [--out <dir>]`: screenshots a plugin's pages in
// headless Chrome, so a page is looked at before it ships. Run it in the plugin's folder. Every preview/<file>.tsx is a
// fixture: a module that renders a page component with made-up data, in one state or in several. A filter is a
// substring of "<plugin>/<file>", which picks every state of those files, or "<plugin>/<file>/<state>", or the start
// of it, for some. <plugin> is the folder's name. Chrome comes from CHROME=/path/to/chrome, else the usual macOS and
// Linux locations. The page's React and wings-plugin-ui come from the plugin's own node_modules, as in a build.
//
// A fixture's exports:
//   default   a React element, or a component, to show
//   props     what a component `default` is shown with
//   element   a React element to show in place of `default`
//   page      { place, fill } as a page declares them, so that `export { default, page } from "../src/pages/x"` is that page.
//             place is "tab" (default), "view", "block", "settings", "sidebar" or "popover": which `show*` mounts it.
//             fill is true for a page that fills the tab (`showTab(..., { fill: true })`): the frame is as tall as --height.
//             Without it the PNG is as tall as the page. A state can set its own `page`
//   project   what `wings.project()` answers (default: a project "web"; null for none)
//   args      what `wings.viewArgs()` answers (default: null)
//   focused   what `wings.focused` is and `wings.onFocus` reports once: true (default), or false for a page out of view
//   panes     what `wings.onPanes` hands its listener once: [{ paneId, cwd, command, project, projectId, session, focused }] (default: none)
//   projects  what `wings.projects()` answers and `wings.onProjects` hands its listener once: [{ id, name, path, branch }] (default: none)
//   storage   { key: value }: what `wings.storage` holds at the start, and keeps what the page sets
//   answers   { name: (request) => response }: what main answers an `ask` with
//   queries   { name: (key) => value, or a value for every key }: what main serves a `useQuery` or `subscribe` with
//   highlight (code, lang) => tokens: what `wings.highlight` answers (default: comments, strings, numbers and keywords colored)
//   states    { name: { ...fields } }: one screenshot per state. Each shows the file's fields with its own over them.
//             The PNGs are <plugin>-<file>-<state>-<width>.png; a file without states makes <plugin>-<file>-<width>.png
// The page gets wings-plugin-ui's plugin-ui.css and the CSS its imports bring, in the app's dark theme, with a stub
// `window.wings` (shot-stub.js). Exits 1 when a fixture throws or logs a console error, 2 without Chrome or a match.
import { spawn } from "node:child_process";
import * as esbuild from "esbuild";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, parse } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const here = import.meta.dirname;

/**
 * Screenshots the fixtures in each folder's preview/ folder. `args` are the command's arguments after `shot`, and
 * `folders` the plugin folders. Resolves the exit code: 0, 1 when a fixture throws or logs a console error, 2 without
 * Chrome or a match. Chrome and the temp files are cleaned up when the process exits.
 */
export async function shot(args, folders) {
  const argv = args.slice();
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
    return 2;
  }

  const chrome = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((p) => p && existsSync(p));
  if (!chrome) {
    console.error("shot needs Chrome: install it or set CHROME=/path/to/chrome.");
    return 2;
  }

  // The fixtures: plugins/<plugin>/preview/<file>.tsx (a file starting with _ is shared data, not one). A state is "<plugin>/<file>/<state>".
  // The states a file has are known once its page has loaded, so a filter that names one picks its file here and the state there.
  const wanted = (id) => !filters.length || filters.some((s) => id.includes(s));
  const fixtures = folders
    .filter((folder) => existsSync(join(folder, "preview")))
    .flatMap((folder) => readdirSync(join(folder, "preview")).filter((f) => /^[^_].*\.(tsx?|jsx?)$/.test(f)).map((f) => ({ id: `${basename(folder)}/${parse(f).name}`, file: join(folder, "preview", f), folder })))
    .filter((f) => wanted(f.id) || filters.some((s) => s.startsWith(`${f.id}/`)));
  if (!fixtures.length) {
    console.error(`No fixture matches ${filters.join(" ") || "anything"}. They are preview/<file>.tsx in the plugin's folder.`);
    return 2;
  }
  mkdirSync(out, { recursive: true });

  // The stylesheet and React come from the plugin's own folder, like the build finds them, so the page and a linked
  // wings-plugin-ui share one React.
  const found = new Map();
  const from = (folder) => {
    if (!found.has(folder)) {
      const need = createRequire(join(folder, "package.json"));
      found.set(folder, { uiCss: readFileSync(need.resolve("wings-plugin-ui/plugin-ui.css"), "utf8"), react: dirname(need.resolve("react/package.json")), reactDom: dirname(need.resolve("react-dom/package.json")) });
    }
    return found.get(folder);
  };
  const work = mkdtempSync(join(tmpdir(), "wings-shot-work-"));

  /** Bundles one fixture, as a page: the fixture, the stub and wings-plugin-ui's `show*`, mounted the way a plugin's own entry does. */
  async function pageOf(fixture) {
    const { uiCss, react, reactDom } = from(fixture.folder);
    const result = await esbuild.build({
      stdin: {
        resolveDir: join(fixture.file, ".."),
        loader: "js",
        contents: `
          import { installWings } from ${JSON.stringify(join(here, "shot-stub.js"))};
          import * as file from ${JSON.stringify(fixture.file)};
          import { createElement } from "react";
          import { showTab, showView, showBlock, showSidebar, showPopover, showSettings } from "wings-plugin-ui";
          // The page shows one state, the one ?state= names or else the file's first, and says which there are.
          const names = Object.keys(file.states ?? {});
          window.fixtureStates = names;
          const fx = names.length ? { ...file, ...file.states[new URLSearchParams(location.search).get("state") ?? names[0]] } : file;
          for (const old of ["place", "fill", "respond"]) if (old in fx) throw new Error("A fixture has no " + old + " any more: say the frame in page = { place, fill }, and what main answers in answers or queries.");
          const place = fx.page?.place ?? "tab";
          const show = { tab: showTab, view: showView, block: showBlock, sidebar: showSidebar, popover: showPopover, settings: showSettings }[place];
          if (!show) throw new Error("place is tab, view, block, settings, sidebar or popover, not " + place);
          const element = fx.element ?? (typeof fx.default === "function" ? createElement(fx.default, fx.props) : fx.default);
          if (element == null) throw new Error("A fixture shows its default export, or a state its element: this one has neither.");
          installWings(fx);
          show(element, { fill: Boolean(fx.page?.fill) });
        `,
      },
      outdir: join(work, fixture.id.replace("/", "-")),
      bundle: true,
      format: "iife",
      jsx: "automatic",
      // React's development build, so its warnings reach the console.
      define: { "process.env.NODE_ENV": '"development"' },
      alias: { react, "react-dom": reactDom },
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

  /** The names of a fixture file's states, which its page says as it loads: none for a file without states. */
  async function statesOf(url) {
    const { targetId } = await send("Target.createTarget", { url });
    const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
    let names;
    for (let i = 0; i < 100 && !names; i++) {
      names = (await send("Runtime.evaluate", { expression: "window.fixtureStates", returnByValue: true }, sessionId)).result.value;
      if (!names) await sleep(100);
    }
    await send("Target.closeTarget", { targetId });
    // A page that threw before it said is shot as one state, so its errors are reported.
    return names ?? [];
  }

  /** Opens the page for `id`, waits for it to draw, and shoots it at each width. Resolves the console errors it logged. */
  async function shoot(id, url) {
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
      const file = join(out, `${id.replaceAll("/", "-")}-${width}.png`);
      writeFileSync(file, Buffer.from(data, "base64"));
      files.push(file);
    }
    drain();
    await send("Target.closeTarget", { targetId });
    return { files, errors };
  }

  console.error(`Screenshots go to ${out}`);
  let failed = 0;
  let shot = 0;
  for (const fixture of fixtures) {
    try {
      const url = await pageOf(fixture);
      const states = await statesOf(url);
      for (const state of states.length ? states : [null]) {
        const id = state === null ? fixture.id : `${fixture.id}/${state}`;
        if (!wanted(id)) continue;
        const { files, errors } = await shoot(id, state === null ? url : `${url}?state=${encodeURIComponent(state)}`);
        shot++;
        for (const f of files) console.log(f);
        if (errors.length) {
          failed++;
          console.error(`${id} logged ${errors.length} console error(s):`);
          for (const e of errors) console.error(`  ${e.split("\n").slice(0, 4).join("\n  ")}`);
        }
      }
    } catch (e) {
      failed++;
      console.error(`${fixture.id} failed: ${e.message ?? e}`);
    }
  }
  if (!shot && !failed) {
    console.error(`No state matches ${filters.join(" ")}. A state is <plugin>/<file>/<state>.`);
    return 2;
  }
  return failed ? 1 : 0;
}
