// `pnpm parity`: renders the same components through the app's shadcn components (parity/app.tsx, the reference) and
// through wings-plugin-ui's plain CSS (parity/plugin.tsx) in headless Chrome, and compares what Chrome computes for
// each: colours (as drawn, over the app's background), radii, sizes, fonts, paddings, borders and shadows, at rest,
// hovered, focused by keyboard, and in the open menu and select. Exits 1 on a difference.
// Opt-in: it needs Chrome (CHROME=/path/to/chrome, else the usual macOS/Linux locations), so CI without one doesn't
// run it. `--shots <dir>` also saves a screenshot of each side. Runs on its own, from anywhere.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { build } from "vite";

const here = fileURLToPath(new URL(".", import.meta.url));
const desktop = join(here, "..");
const shotsAt = process.argv.includes("--shots") ? process.argv[process.argv.indexOf("--shots") + 1] : null;

const chrome = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((p) => p && existsSync(p));
if (!chrome) {
  console.error("parity needs Chrome: install it or set CHROME=/path/to/chrome.");
  process.exit(2);
}

// What is compared, per element: `sel` is the same on both pages, each page marks its cases with data-case.
// States: base, hover (pointer over it), focus (keyboard focus).
// `skip` is a pattern of properties not compared. The select popup's padding: the app pads the group inside it, Wings the
// popup, and the option's offset in the popup (left, top) is compared, which is what shows.
const probes = [
  ...["default", "secondary", "outline", "ghost", "destructive", "link"].map((v) => ({ name: `button ${v}`, sel: `[data-case="button-${v}"]`, states: ["base", "hover", "focus"] })),
  ...["default", "sm", "xs", "lg"].map((s) => ({ name: `button size ${s}`, sel: `[data-case="button-size-${s}"]`, states: ["base"] })),
  ...["default", "secondary", "destructive", "outline", "ghost", "link"].map((v) => ({ name: `badge ${v}`, sel: `[data-case="badge-${v}"]`, states: ["base"] })),
  { name: "input", sel: '[data-case="input"]', states: ["base", "hover", "focus"] },
  { name: "input with text", sel: '[data-case="input-value"]', states: ["base"] },
  { name: "tabs list", sel: '[data-case="tabs-default"] [role="tablist"]', states: ["base"] },
  { name: "tab active", sel: '[data-case="tabs-default"] [role="tab"][aria-selected="true"]', states: ["base", "focus"] },
  { name: "tab idle", sel: '[data-case="tabs-default"] [role="tab"][aria-selected="false"]', states: ["base", "hover"] },
  { name: "line tabs list", sel: '[data-case="tabs-line"] [role="tablist"]', states: ["base"] },
  { name: "line tab active", sel: '[data-case="tabs-line"] [role="tab"][aria-selected="true"]', states: ["base"] },
  { name: "line tab idle", sel: '[data-case="tabs-line"] [role="tab"][aria-selected="false"]', states: ["base"] },
  { name: "chip off", sel: '[data-case="chip"]', states: ["base", "hover"] },
  { name: "chip on", sel: '[data-case="chip-on"]', states: ["base"] },
  { name: "select trigger", sel: '[data-case="select"] [role="combobox"]', states: ["base", "hover", "focus"] },
  { name: "menu trigger", sel: '[data-case="menu"] button', states: ["base", "hover"] },
];
// Open popups: click `open`, then compare each part, then Escape.
const popups = [
  {
    open: '[data-case="select"] [role="combobox"]',
    parts: [
      { name: "select popup", sel: '*:has(> [role="listbox"])', states: ["base"], skip: /^padding/ },
      { name: "select item", sel: '[role="option"]:not([aria-selected="true"])', states: ["base", "hover"] },
      { name: "select item chosen", sel: '[role="option"][aria-selected="true"]', states: ["base"] },
    ],
  },
  {
    open: '[data-case="menu"] button',
    parts: [
      { name: "menu popup", sel: '[role="menu"]', states: ["base"] },
      { name: "menu item", sel: '[role="menuitem"]:first-child', states: ["base", "hover"] },
      { name: "menu item destructive", sel: '[role="menuitem"]:last-child', states: ["base", "hover"] },
    ],
  },
];

const PROPS = ["color", "backgroundColor", "backgroundClip", "borderTopColor", "borderRightColor", "borderBottomColor", "borderLeftColor", "borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth", "borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius", "fontSize", "fontWeight", "lineHeight", "letterSpacing", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "columnGap", "opacity", "boxShadow", "textDecorationLine", "textTransform"];
const COLOR = /color|boxShadow/i;
const COLOR_TOLERANCE = 2; // of 255, per channel
const LENGTH_TOLERANCE = 0.6; // px

// ---------- the pages ----------

const out = mkdtempSync(join(tmpdir(), "wings-parity-"));
process.on("exit", () => rmSync(out, { recursive: true, force: true }));
await build({
  root: desktop,
  logLevel: "warn",
  configFile: false,
  plugins: [react(), tailwindcss()],
  // wings-plugin-ui is linked and has its own React in node_modules: one copy, the app's, or hooks break.
  resolve: { alias: { "@": join(desktop, "src") }, dedupe: ["react", "react-dom"] },
  build: { outDir: out, emptyOutDir: true, rollupOptions: { input: { app: join(here, "app.html"), plugin: join(here, "plugin.html") } } },
});
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };
const server = createServer((req, res) => {
  const path = join(out, new URL(req.url, "http://x").pathname);
  if (!path.startsWith(out) || !existsSync(path)) return res.writeHead(404).end();
  res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(readFileSync(path));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const origin = `http://127.0.0.1:${server.address().port}`;

// ---------- Chrome over the DevTools protocol ----------

const profile = mkdtempSync(join(tmpdir(), "wings-parity-chrome-"));
const proc = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--hide-scrollbars", "--force-device-scale-factor=2", "--window-size=600,800", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
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
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.id && waiting.has(m.id)) {
    const { resolve, reject } = waiting.get(m.id);
    waiting.delete(m.id);
    m.error ? reject(new Error(m.error.message)) : resolve(m.result);
  }
};
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    waiting.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function openPage(file) {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  await call("Page.enable");
  await call("Runtime.enable");
  const errors = [];
  ws.addEventListener("message", ({ data }) => {
    const m = JSON.parse(data);
    if (m.sessionId === sessionId && m.method === "Runtime.exceptionThrown") errors.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
  });
  await call("Emulation.setDeviceMetricsOverride", { width: 800, height: 560, deviceScaleFactor: 2, mobile: false });
  await call("Page.navigate", { url: `${origin}/${file}` });
  for (let i = 0; i < 100; i++) {
    if ((await evaluate(call, "document.querySelector('[data-case]') !== null")) === true) break;
    await sleep(100);
  }
  if (errors.length) throw new Error(`${file} threw: ${errors.join("\n")}`);
  await sleep(300);
  return call;
}
async function evaluate(call, expression) {
  const { result, exceptionDetails } = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
}

/** What Chrome computes for `sel`, in numbers a diff can compare: colours as drawn over the app's background. */
const measure = (call, sel) =>
  evaluate(
    call,
    `(() => {
      const el = document.querySelector(${JSON.stringify(sel)});
      if (!el) return null;
      const cs = getComputedStyle(el);
      const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
      const drawn = (color) => {
        ctx.fillStyle = "#0f0e12"; ctx.fillRect(0, 0, 1, 1);
        ctx.fillStyle = color; ctx.fillRect(0, 0, 1, 1);
        return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
      };
      const colors = (v) => v.replace(/(?:rgba?|color|oklab|oklch|lab|lch|hsla?)\\([^)]*\\)/g, (c) => "{" + drawn(c).join(",") + "}");
      const r = el.getBoundingClientRect();
      // How far an option sits inside its popup, since the app pads a select's list in a group and Wings in the popup.
      const pop = el.closest('[role="menu"]') ?? el.closest('[role="listbox"]')?.parentElement;
      const o = pop && pop !== el ? { left: Math.round((r.left - pop.getBoundingClientRect().left) * 100) / 100 + "px", top: Math.round((r.top - pop.getBoundingClientRect().top) * 100) / 100 + "px" } : {};
      Object.assign(o, { width: Math.round(r.width * 100) / 100 + "px", height: Math.round(r.height * 100) / 100 + "px" });
      // Font props mean nothing on a box with no text of its own, like a tab list or a popup: they only pass on to children.
      const text = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) || el.matches("input");
      for (const p of ${JSON.stringify(PROPS)}) {
        let v = cs[p];
        if (/^(fontSize|fontWeight|lineHeight|letterSpacing)$/.test(p) && !text) continue;
        // A border colour only shows where there is a border; "normal" gap is none; shadow layers of nothing draw nothing.
        if (/^border(Top|Right|Bottom|Left)Color$/.test(p) && cs[p.replace("Color", "Width")] === "0px") continue;
        if (p === "columnGap" && v === "normal") v = "0px";
        if (p === "boxShadow" && v !== "none") v = colors(v);
        o[p] = /color/i.test(p) ? colors(v) : v;
      }
      return o;
    })()`,
  );
const center = async (call, sel) => evaluate(call, `(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) throw new Error("no element for " + ${JSON.stringify(sel)}); const r = e.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
// Hover and focus colours fade in, so a sample taken mid-transition reads a colour between the two.
const settle = async (call) => {
  await sleep(50);
  await evaluate(call, "Promise.all(document.getAnimations().map((a) => a.finished)).then(() => true)");
};
const hover = async (call, sel) => {
  const [x, y] = await center(call, sel);
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await settle(call);
};
const unhover = async (call) => {
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 5, y: 5 });
  await settle(call);
};
const click = async (call, sel) => {
  const [x, y] = await center(call, sel);
  for (const type of ["mousePressed", "mouseReleased"]) await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  await sleep(400);
};
const focus = async (call, sel) => {
  // A key press first, so :focus-visible matches like it does for someone tabbing.
  for (const type of ["keyDown", "keyUp"]) await call("Input.dispatchKeyEvent", { type, key: "Shift", code: "ShiftLeft", windowsVirtualKeyCode: 16 });
  await evaluate(call, `document.querySelector(${JSON.stringify(sel)}).focus()`);
  await settle(call);
};
const blur = (call) => evaluate(call, "document.activeElement?.blur()");

/** Every state of every probe on one page: { "button default": { base: {...}, hover: {...} } }. */
async function collect(file, shot) {
  const call = await openPage(file);
  const got = {};
  const record = async (p, sel = p.sel) => {
    got[p.name] = {};
    for (const state of p.states) {
      if (state === "hover") await hover(call, sel);
      if (state === "focus") await focus(call, sel);
      got[p.name][state] = await measure(call, sel);
      if (state === "hover") await unhover(call);
      if (state === "focus") await blur(call);
    }
  };
  for (const p of probes) await record(p);
  if (shot) {
    const { data } = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(`${shot}-page.png`, Buffer.from(data, "base64"));
  }
  for (const group of popups) {
    await click(call, group.open);
    for (const p of group.parts) await record(p);
    if (shot) {
      const { data } = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(`${shot}-${group.parts[0].name.split(" ")[0]}-open.png`, Buffer.from(data, "base64"));
    }
    await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await sleep(300);
  }
  return got;
}

/** A shadow without its layers of nothing, which draw nothing but are how Tailwind writes a ring that is off. */
const drawn = (v) => (typeof v === "string" && v.includes("{") && v.includes(", {") ? v.split(/, (?=\{)/).filter((l) => !/^\{[\d,]+\} 0px 0px 0px 0px$/.test(l)).join(", ") || "none" : v);
const same = (prop, a, b) => {
  if (prop === "boxShadow") [a, b] = [drawn(a), drawn(b)];
  if (a === b) return true;
  if (typeof a !== "string" || typeof b !== "string") return false;
  if (COLOR.test(prop)) {
    const ca = a.match(/\{[\d,]+\}/g);
    const cb = b.match(/\{[\d,]+\}/g);
    if (!ca || !cb || ca.length !== cb.length || a.replace(/\{[\d,]+\}/g, "") !== b.replace(/\{[\d,]+\}/g, "")) {
      // Shadows: the lengths around the colours must match too, the colours within the tolerance.
      if (!ca || !cb || ca.length !== cb.length) return false;
    }
    return ca.every((c, i) => c.slice(1, -1).split(",").every((n, k) => Math.abs(n - cb[i].slice(1, -1).split(",")[k]) <= COLOR_TOLERANCE)) && a.replace(/\{[\d,]+\}/g, "{}") === b.replace(/\{[\d,]+\}/g, "{}");
  }
  const pa = a.split(" ").map(parseFloat);
  const pb = b.split(" ").map(parseFloat);
  return a.endsWith("px") && pa.length === pb.length && pa.every((n, i) => Math.abs(n - pb[i]) <= LENGTH_TOLERANCE);
};

let failed = 0;
try {
  if (shotsAt) mkdirSync(shotsAt, { recursive: true });
  const app = await collect("parity/app.html", shotsAt && join(shotsAt, "app"));
  const plugin = await collect("parity/plugin.html", shotsAt && join(shotsAt, "plugin"));
  const skips = new Map([...probes, ...popups.flatMap((g) => g.parts)].filter((p) => p.skip).map((p) => [p.name, p.skip]));
  let checked = 0;
  for (const name of Object.keys(app)) {
    for (const state of Object.keys(app[name])) {
      const a = app[name][state];
      const b = plugin[name]?.[state];
      if (!a || !b) {
        console.log(`${name} (${state}): missing on the ${a ? "plugin" : "app"} side`);
        failed++;
        continue;
      }
      const props = ["left", "top", "width", "height", ...PROPS];
      for (const p of props) {
        if (skips.get(name)?.test(p)) continue;
        checked++;
        if (!same(p, a[p], b[p])) {
          failed++;
          console.log(`${name} (${state}) ${p}: app ${a[p]}, plugin-ui ${b[p]}`);
        }
      }
    }
  }
  console.log(failed ? `\nparity: ${failed} difference${failed === 1 ? "" : "s"} in ${checked} values` : `parity: ${checked} values match`);
} finally {
  const exited = new Promise((r) => proc.on("exit", r));
  proc.kill();
  await exited;
  server.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
process.exit(failed ? 1 : 0);
