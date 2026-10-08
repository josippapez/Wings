/**
 * Terminal rendering benchmark, run inside the real webview with `WINGS_BENCH=1`.
 * Synthetic load, no PTY: it measures xterm.js + the webview, which is what limits busy panes.
 */
import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

type Renderer = "webgl" | "dom";
const SECONDS = 5;
const words = "the quick claude agent reads files edits code runs tests and reports back with a diff".split(" ");

/** A full-screen repaint like Claude Code's fullscreen renderer: synchronized output, colors, cursor home. */
function frames(cols: number, rows: number): string[] {
  return Array.from({ length: 24 }, (_, n) => {
    let s = "\x1b[?2026h\x1b[H";
    for (let r = 0; r < rows; r++) {
      let line = "";
      for (let i = r + n; line.length < cols - 12; i++) line += words[i % words.length] + " ";
      s += `\x1b[38;5;${((r * 7 + n) % 230) + 1}m${line.slice(0, cols - 1)}\x1b[K`;
      if (r < rows - 1) s += "\r\n";
    }
    return s + "\x1b[0m\x1b[?2026l";
  });
}

function stats(xs: number[]) {
  const s = [...xs].sort((a, b) => a - b);
  const at = (p: number) => Math.round((s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0) * 10) / 10;
  return { n: s.length, p50: at(0.5), p95: at(0.95), max: at(1) };
}

function makeTerminals(root: HTMLElement, count: number, renderer: Renderer) {
  root.replaceChildren();
  root.style.cssText = `position:fixed;inset:0;display:grid;grid-template-columns:repeat(${count > 1 ? 2 : 1},1fr);gap:8px;padding:8px;background:#141217`;
  return Array.from({ length: count }, () => {
    const el = document.createElement("div");
    el.style.cssText = "min-height:0;min-width:0;overflow:hidden";
    root.appendChild(el);
    const term = new Terminal({ fontSize: 13, lineHeight: 1.2, scrollback: 10_000, theme: { background: "#17151b" } });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    if (renderer === "webgl") term.loadAddon(new WebglAddon());
    fit.fit();
    return term;
  });
}

/** Records the gap between animation frames until stopped. */
function frameClock() {
  const gaps: number[] = [];
  let last = performance.now();
  let on = true;
  const tick = (t: number) => {
    gaps.push(t - last);
    last = t;
    if (on) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  return () => {
    on = false;
    return gaps.slice(1);
  };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Every pane repaints as fast as xterm accepts data. */
async function redrawFlood(terms: Terminal[]) {
  let bytes = 0;
  const end = performance.now() + SECONDS * 1000;
  const stop = frameClock();
  await Promise.all(
    terms.map(
      (term) =>
        new Promise<void>((done) => {
          const fs = frames(term.cols, term.rows);
          let i = 0;
          const next = () => {
            if (performance.now() > end) return done();
            const f = fs[i++ % fs.length];
            bytes += f.length;
            term.write(f, next);
          };
          next();
        }),
    ),
  );
  const gaps = stop();
  return { mbPerSec: Math.round((bytes / SECONDS / 1e6) * 10) / 10, frameMs: stats(gaps), longFrames: gaps.filter((g) => g > 50).length };
}

/** Pane 1 echoes a keystroke every 50 ms while the others repaint at 30 fps; measures write-to-paint time. */
async function typingUnderLoad(terms: Terminal[]) {
  const [typing, ...busy] = terms;
  const busyFrames = busy.map((t) => frames(t.cols, t.rows));
  let n = 0;
  const load = setInterval(() => busy.forEach((t, i) => t.write(busyFrames[i][n++ % 24])), 33);
  const samples: number[] = [];
  const end = performance.now() + SECONDS * 1000;
  while (performance.now() < end) {
    const start = performance.now();
    await new Promise<void>((done) => {
      const sub = typing.onRender(() => {
        sub.dispose();
        samples.push(performance.now() - start);
        done();
      });
      typing.write("x");
    });
    await sleep(50);
  }
  clearInterval(load);
  return { echoMs: stats(samples) };
}

/** One pane takes `cat`-style output: 16 MB of plain lines. */
async function bulk(term: Terminal) {
  const line = "drwxr-xr-x  12 user  staff   384 Oct  8 14:02 node_modules/some-package/dist/index.js\r\n";
  const chunk = line.repeat(Math.ceil(65536 / line.length));
  const total = 16 * 1024 * 1024;
  const start = performance.now();
  let sent = 0;
  await new Promise<void>((done) => {
    const next = () => {
      if (sent >= total) return done();
      sent += chunk.length;
      term.write(chunk, next);
    };
    next();
  });
  const secs = (performance.now() - start) / 1000;
  return { mbPerSec: Math.round((sent / secs / 1e6) * 10) / 10 };
}

export async function runBench() {
  const root = document.getElementById("root")!;
  const report: Record<string, unknown> = { userAgent: navigator.userAgent, seconds: SECONDS };
  for (const renderer of ["webgl", "dom"] as Renderer[]) {
    const four = makeTerminals(root, 4, renderer);
    await sleep(300);
    const redraw = await redrawFlood(four);
    four.forEach((t) => t.reset());
    const typing = await typingUnderLoad(four);
    four.forEach((t) => t.dispose());
    const [one] = makeTerminals(root, 1, renderer);
    await sleep(300);
    const cat = await bulk(one);
    one.dispose();
    report[renderer] = { fourPanesRedraw: redraw, typingWhileThreeRedraw: typing, singlePaneCat: cat };
  }
  await invoke("bench_report", { report: JSON.stringify(report, null, 2) });
}
