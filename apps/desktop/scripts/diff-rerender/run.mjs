// `node scripts/diff-rerender/run.mjs`: opens the diff viewer on a 3000-line file in headless Chrome, scrolls into it,
// renders it again ten times the way App does whenever anything in Wings changes, and checks the open file's rows were
// left alone and the scroll stayed. @pierre/diffs redraws every row when its options or annotations are new objects,
// which once happened on every App render. Exits 1 when a re-render touched the rows or moved the scroll.
// Opt-in: it needs Chrome (CHROME=/path/to/chrome, else the usual macOS/Linux locations).
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { build } from "vite";

const here = import.meta.dirname;
const desktop = join(here, "../..");
const chrome = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((p) => p && existsSync(p));
if (!chrome) {
  console.error("diff-rerender needs Chrome: install it or set CHROME=/path/to/chrome.");
  process.exit(2);
}

const out = mkdtempSync(join(tmpdir(), "wings-diff-rerender-"));
const profile = mkdtempSync(join(tmpdir(), "wings-diff-rerender-chrome-"));
let proc;
process.on("exit", () => {
  proc?.kill();
  rmSync(out, { recursive: true, force: true });
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

await build({
  root: desktop,
  logLevel: "warn",
  configFile: false,
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": join(desktop, "src") }, dedupe: ["react", "react-dom"] },
  build: { outDir: out, emptyOutDir: true, rollupOptions: { input: { page: join(here, "page.html") } } },
});
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };
const server = createServer((req, res) => {
  const path = join(out, new URL(req.url, "http://x").pathname);
  if (!path.startsWith(out) || !existsSync(path)) return res.writeHead(404).end();
  res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(readFileSync(path));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const origin = `http://127.0.0.1:${server.address().port}`;

proc = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
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
const errors = [];
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.id && waiting.has(m.id)) {
    const { resolve, reject } = waiting.get(m.id);
    waiting.delete(m.id);
    m.error ? reject(new Error(m.error.message)) : resolve(m.result);
  } else if (m.method === "Runtime.exceptionThrown") errors.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
};
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    waiting.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const { targetId } = await send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
const call = (method, params) => send(method, params, sessionId);
const evaluate = async (expression) => {
  const { result, exceptionDetails } = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
};
await call("Runtime.enable");
await call("Emulation.setDeviceMetricsOverride", { width: 1400, height: 900, deviceScaleFactor: 1, mobile: false });
await call("Page.navigate", { url: `${origin}/scripts/diff-rerender/page.html` });
const until = async (expression, what) => {
  for (let i = 0; i < 150; i++) {
    if ((await evaluate(expression)) === true) return;
    await sleep(100);
  }
  throw new Error(`Timed out waiting for ${what}${errors.length ? `:\n${errors.join("\n")}` : ""}`);
};

// The diff's scroller is the Virtualizer, the one element whose rows are the file's.
const scroller = `document.querySelector("diffs-container")?.closest(".overflow-auto")`;
await until("Boolean(window.__check)", "the page");
await sleep(300);
await evaluate("window.__check.setPatch()");
await until(`(${scroller})?.scrollHeight > 20000`, "the diff to draw");
await sleep(1500);
await evaluate(`(${scroller}).scrollTop = 20000`);
await sleep(1000);
await evaluate(`(() => {
  window.__touched = 0;
  new MutationObserver((records) => (window.__touched += records.reduce((n, r) => n + r.addedNodes.length + r.removedNodes.length, 0)))
    .observe(document.querySelector("diffs-container").shadowRoot, { childList: true, subtree: true });
})()`);
// Wrapped lines get measured once they draw, which moves the scroll a little; compare against where it settled.
const before = await evaluate(`(${scroller}).scrollTop`);
for (let i = 0; i < 10; i++) {
  await evaluate("window.__check.rerender()");
  await sleep(150);
}
const touched = await evaluate("window.__touched");
const top = await evaluate(`(${scroller}).scrollTop`);
console.log(`10 re-renders: ${touched} row nodes added or removed, scrollTop ${before} -> ${top}`);
if (errors.length) console.error(errors.join("\n"));
process.exit(touched === 0 && top === before && !errors.length ? 0 : 1);
