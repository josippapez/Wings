// `pnpm repaint`: builds the app, runs it in a real WKWebView (host.swift) with three test plugins badging every pane,
// splits a tab, opens a second one and switches back and forth quickly, then checks the badges were drawn, pixel by
// pixel against the first switch. WebKit's headless test builds paint screenshots afresh, so they never show a layer
// WebKit forgot to repaint; this captures what the window server composited. Opt-in: macOS only, needs swiftc and
// Screen Recording for the terminal. A borderless window sits at desktop level for about 20 s and never takes focus.
// `--keep` keeps the screenshots. Exits 1 when a badge row was left undrawn.
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { build } from "vite";

if (process.platform !== "darwin") {
  console.error("repaint runs WKWebView, so macOS only.");
  process.exit(2);
}
const here = import.meta.dirname;
const desktop = join(here, "..", "..");
const keep = process.argv.includes("--keep");
const work = mkdtempSync(join(tmpdir(), "wings-repaint-"));
process.on("exit", () => keep || rmSync(work, { recursive: true, force: true }));

const dist = join(work, "dist");
await build({
  root: desktop,
  logLevel: "warn",
  configFile: false,
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": join(desktop, "src") } },
  build: { outDir: dist, emptyOutDir: true, chunkSizeWarningLimit: Infinity },
});
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };
const server = createServer((req, res) => {
  const pathname = new URL(req.url, "http://x").pathname;
  const path = join(dist, pathname === "/" ? "index.html" : pathname);
  if (!path.startsWith(dist) || !existsSync(path)) return res.writeHead(404).end();
  res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(readFileSync(path));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));

const host = join(work, "host");
execFileSync("swiftc", ["-O", "-o", host, join(here, "host.swift")], { stdio: "inherit" });
const shots = join(work, "shots");
mkdirSync(shots);
const code = await new Promise((resolve) => {
  const proc = spawn(host, [`http://127.0.0.1:${server.address().port}/`, join(here, "stub.js"), join(here, "plugin.js"), shots], { stdio: "inherit" });
  proc.on("exit", resolve);
});
server.close();
if (keep) console.log(`Screenshots: ${shots}`);
process.exit(code ?? 2);
