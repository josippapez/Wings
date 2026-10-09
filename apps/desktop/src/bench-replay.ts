/**
 * Renderer benchmark (`WINGS_BENCH=replay`): a recorded Claude Code session (fullscreen renderer, alt screen, mouse
 * modes; `bench/claude-fullscreen.rec`) replayed into 1, 4 and 12 panes, and 12 with only 4 shown, for one renderer
 * per run: xterm.js DOM, xterm.js WebGL, or rows parsed in Rust drawn on a canvas per pane or on one shared canvas.
 * Run every renderer with `scripts/bench-renderers.sh`.
 *
 * In Chromium (`.claude/skills/wings-performance/bench-stub.js`) there is no Rust: the bytes, and the frames that
 * `bench-grid` encoded ahead of time, are played from JavaScript, so Rust's work is missing from those numbers.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { CanvasPane, FONT_FAMILY } from "./bench-canvas";

export type Renderer = "dom" | "webgl" | "canvas" | "canvas-shared";
type Recording = { cols: number; rows: number; bytes: number; ms: number };
type ReplayPlan = { grid: boolean; visible: boolean[]; fromMs: number[]; playMs?: number };
type ReplayStats = { bytes: number; messages: number; ms: number };
type ProcessSample = {
  pid: number;
  cpuMs: number;
  footprintMb?: number;
  ownedGraphicsMb?: number;
  ownedGraphicsSwappedMb?: number;
  ownedGraphicsRegions?: number;
  allGraphicsMb?: number;
};
export type Sample = { wings: ProcessSample; webContent: ProcessSample | null; gpu: ProcessSample | null };

/** Where Claude streams its answer in the recording; panes start 400 ms apart so they don't draw in lockstep. */
const PACED_FROM_MS = 40_000;
const PACED_MS = 5_000;
const CONFIGS: { panes: number; visible: number; scrollback?: number; only?: Renderer[] }[] = [
  { panes: 1, visible: 1 },
  { panes: 4, visible: 4 },
  { panes: 12, visible: 4 },
  { panes: 12, visible: 12 },
  // A long session: the scrollback is full and the glyph atlas has seen many colors.
  { panes: 4, visible: 4, scrollback: 10_000, only: ["webgl"] },
];

/** `count` lines of colored build and log output, each line in another of the 256 colors. */
function scrollbackLines(count: number) {
  const words = "compiling wings v0.1.0 src/lib.rs:42 warning unused variable PASS FAIL 12.4ms ✓ ✗ → node_modules".split(" ");
  let s = "";
  for (let i = 0; i < count; i++) {
    s += `\x1b[38;5;${i % 256}m${String(i).padStart(5)} `;
    for (let w = 0; w < 9; w++) s += (w === 3 ? "\x1b[1m" : "") + words[(i * 7 + w * 3) % words.length] + "\x1b[22m ";
    s += "\x1b[0m\r\n";
  }
  return new TextEncoder().encode(s);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const round = (n: number, digits = 1) => Math.round(n * 10 ** digits) / 10 ** digits;

export function stats(xs: number[]) {
  const s = [...xs].sort((a, b) => a - b);
  const at = (p: number) => round(s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0);
  return { n: s.length, p50: at(0.5), p95: at(0.95), max: at(1) };
}

/** Records the gap between animation frames until stopped. */
export function frameClock() {
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
    const g = gaps.slice(1);
    return { ...stats(g), over50ms: g.filter((x) => x > 50).length };
  };
}

export const sample = (memory: boolean) => invoke<Sample>("bench_sample", { memory });

/** CPU use of each process between two samples, in percent of one core. */
export function cpuPercent(a: Sample, b: Sample, ms: number) {
  const pct = (x?: ProcessSample | null, y?: ProcessSample | null) => (x && y ? round(((y.cpuMs - x.cpuMs) / ms) * 100) : null);
  return { wings: pct(a.wings, b.wings), webContent: pct(a.webContent, b.webContent), gpu: pct(a.gpu, b.gpu) };
}

interface Pane {
  write(data: Uint8Array): void;
  /** Resolves once everything written so far is parsed and drawn. */
  flushed(): Promise<void>;
  reset(): void;
  dispose(): void;
}

class XtermPane implements Pane {
  private term: Terminal;
  private webgl: WebglAddon | null = null;
  constructor(host: HTMLElement, cols: number, rows: number, fontSize: number, webgl: boolean) {
    this.term = new Terminal({ cols, rows, fontSize, fontFamily: FONT_FAMILY, lineHeight: 1.2, scrollback: 10_000, theme: { background: "#17151b", foreground: "#e9e6e1" } });
    this.term.open(host);
    if (webgl) {
      this.webgl = new WebglAddon();
      this.term.loadAddon(this.webgl);
    }
  }
  write(data: Uint8Array) {
    this.term.write(data);
  }
  flushed() {
    return new Promise<void>((done) => this.term.write("", () => requestAnimationFrame(() => done())));
  }
  reset() {
    this.term.reset();
  }
  dispose() {
    // As `TerminalSession.disableWebgl`: lose the context now rather than when the canvas is collected.
    const canvases = [...(this.term.element?.querySelectorAll("canvas") ?? [])];
    this.webgl?.dispose();
    for (const c of canvases) c.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
    this.term.dispose();
  }
}

class Canvas implements Pane {
  constructor(
    private pane: CanvasPane,
    private canvas: HTMLCanvasElement | null,
  ) {}
  write(data: Uint8Array) {
    this.pane.write(data);
  }
  flushed() {
    return this.pane.flushed();
  }
  reset() {
    this.pane.reset();
  }
  dispose() {
    this.canvas?.remove();
  }
}

function canvasFor(width: number, height: number) {
  const canvas = document.createElement("canvas");
  const dpr = devicePixelRatio;
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  canvas.style.cssText = `position:absolute;left:0;top:0;width:${width}px;height:${height}px`;
  const ctx = canvas.getContext("2d", { alpha: false })!;
  ctx.scale(dpr, dpr);
  ctx.fillStyle = "#141217";
  ctx.fillRect(0, 0, width, height);
  return { canvas, ctx };
}

/**
 * Lays out the visible panes in a grid (1, 2x2 or 4x3) and the hidden ones as a second tab, the way Wings hides an
 * inactive tab (`visibility: hidden`). Every pane has the recording's size, so the font shrinks to fit.
 */
function layout(root: HTMLElement, count: number, visibleCount: number, renderer: Renderer, rec: Recording) {
  root.replaceChildren();
  root.style.cssText = "position:fixed;inset:0;background:#141217";
  const columns = visibleCount === 1 ? 1 : visibleCount === 4 ? 2 : 4;
  const tab = (visible: boolean) => {
    const el = document.createElement("div");
    el.style.cssText = `position:absolute;inset:0;display:grid;grid-template-columns:repeat(${columns},1fr);grid-auto-rows:1fr;gap:8px;padding:8px;${visible ? "" : "visibility:hidden"}`;
    root.appendChild(el);
    return el;
  };
  const shown = tab(true);
  const hidden = count > visibleCount ? tab(false) : null;
  const cells = Array.from({ length: count }, (_, i) => {
    const el = document.createElement("div");
    el.style.cssText = "position:relative;min-height:0;min-width:0;overflow:hidden;background:#17151b;border-radius:10px";
    (i < visibleCount ? shown : hidden!).appendChild(el);
    return el;
  });
  const box = cells[0].getBoundingClientRect();
  const fontSize = Math.floor(Math.min((box.width - 8) / (rec.cols * 0.61), (box.height - 8) / (rec.rows * 1.2)) * 2) / 2;
  const shared = renderer === "canvas-shared" ? canvasFor(innerWidth, innerHeight) : null;
  if (shared) root.appendChild(shared.canvas);
  const panes: Pane[] = cells.map((cell, i) => {
    if (renderer === "dom" || renderer === "webgl") return new XtermPane(cell, rec.cols, rec.rows, fontSize, renderer === "webgl" && i < visibleCount);
    const r = cell.getBoundingClientRect();
    if (shared) return new Canvas(new CanvasPane(shared.ctx, { x: r.x + 4, y: r.y + 4 }, rec.cols, rec.rows, fontSize), null);
    const own = canvasFor(r.width, r.height);
    cell.appendChild(own.canvas);
    return new Canvas(new CanvasPane(own.ctx, { x: 4, y: 4 }, rec.cols, rec.rows, fontSize), own.canvas);
  });
  return { panes, fontSize };
}

/** Plays the recording into `panes`; resolves when Rust has sent everything and every pane has drawn it. */
async function replay(panes: Pane[], plan: ReplayPlan, browser: BrowserSource | null) {
  const resolvers: (() => void)[] = [];
  const done = panes.map((_, i) => new Promise<void>((r) => (resolvers[i] = r)));
  const onMessage = (i: number) => (buf: ArrayBuffer) => {
    if (buf.byteLength === 0) void panes[i].flushed().then(resolvers[i]);
    else panes[i].write(new Uint8Array(buf));
  };
  const started = performance.now();
  const sent = browser
    ? await browser.play(panes.map((_, i) => onMessage(i)), plan)
    : await invoke<ReplayStats>("bench_replay", {
        channels: panes.map((_, i) => {
          const channel = new Channel<ArrayBuffer>();
          channel.onmessage = onMessage(i);
          return channel;
        }),
        plan,
      });
  await Promise.all(done);
  const ms = performance.now() - started;
  return { ms: round(ms, 0), channelMb: round(sent.bytes / 1e6, 2), messages: sent.messages, channelMbPerSec: round(sent.bytes / 1e6 / (ms / 1000), 2) };
}

/** Chromium stand-in for `bench_replay`: the same bytes and frames, timed with setTimeout. */
class BrowserSource {
  private constructor(
    private chunks: [number, Uint8Array][],
    private frames: [number, Uint8Array][],
    readonly rec: Recording,
  ) {}

  static async load(): Promise<BrowserSource> {
    const read = async (url: string, header: number) => {
      const buf = new Uint8Array(await (await fetch(url)).arrayBuffer());
      const view = new DataView(buf.buffer);
      const out: [number, Uint8Array][] = [];
      for (let i = header; i < buf.length; ) {
        const n = view.getUint32(i + 4, true);
        out.push([view.getUint32(i, true), buf.subarray(i + 8, i + 8 + n)]);
        i += 8 + n;
      }
      return { out, view };
    };
    const rec = await read("/bench/claude-fullscreen.rec", 4);
    const frames = await read("/bench/claude-fullscreen.frames", 0);
    const bytes = rec.out.reduce((s, c) => s + c[1].length, 0);
    return new BrowserSource(rec.out, frames.out, { cols: rec.view.getUint16(0, true), rows: rec.view.getUint16(2, true), bytes, ms: rec.out.at(-1)![0] });
  }

  async play(sinks: ((buf: ArrayBuffer) => void)[], plan: ReplayPlan): Promise<ReplayStats> {
    const stats = { bytes: 0, messages: 0, ms: 0 };
    const send = (i: number, data: Uint8Array) => {
      stats.bytes += data.length;
      stats.messages++;
      sinks[i](data.slice().buffer);
    };
    await Promise.all(
      sinks.map(async (sink, i) => {
        const items = plan.grid ? this.frames : this.chunks;
        const from = plan.fromMs[i];
        const show = !plan.grid || plan.visible[i];
        const before = items.filter((c) => c[0] < from);
        // The fast-forward arrives at once: raw bytes as one message, frames each as they were.
        if (show && plan.grid) before.forEach((c) => send(i, c[1]));
        else if (show && before.length) send(i, concat(before.map((c) => c[1])));
        const after = items.filter((c) => c[0] >= from && (plan.playMs === undefined || c[0] < from + plan.playMs));
        const start = performance.now();
        for (let k = 0; k < after.length; ) {
          if (plan.playMs === undefined) {
            // Drain: 64 KB of input per task, like `pty.rs` reads when output piles up.
            const batch: Uint8Array[] = [];
            for (let size = 0; k < after.length && size < 65536; k++) {
              batch.push(after[k][1]);
              size += after[k][1].length;
            }
            if (show && plan.grid) batch.forEach((b) => send(i, b));
            else if (show) send(i, concat(batch));
            await sleep(0);
          } else {
            await sleep(after[k][0] - from - (performance.now() - start));
            if (show) send(i, after[k][1]);
            k++;
          }
        }
        if (plan.playMs !== undefined) await sleep(plan.playMs - (performance.now() - start));
        sink(new ArrayBuffer(0));
      }),
    );
    return stats;
  }
}

function concat(parts: Uint8Array[]) {
  const out = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
  let at = 0;
  for (const p of parts) (out.set(p, at), (at += p.length));
  return out;
}

export async function runReplay(renderer: Renderer, native: boolean) {
  const root = document.getElementById("root")!;
  const browser = native ? null : await BrowserSource.load();
  const rec = browser?.rec ?? (await invoke<Recording>("bench_recording"));
  const grid = renderer === "canvas" || renderer === "canvas-shared";
  await sleep(500);
  const report: Record<string, unknown> = {
    renderer,
    webview: native ? "WKWebView" : "Chromium",
    userAgent: navigator.userAgent,
    devicePixelRatio,
    window: [innerWidth, innerHeight],
    recording: rec,
    runs: [] as unknown[],
  };
  for (const { panes: count, visible: visibleCount, scrollback, only } of CONFIGS) {
    if (only && !only.includes(renderer)) continue;
    const { panes, fontSize } = layout(root, count, visibleCount, renderer, rec);
    const visible = panes.map((_, i) => i < visibleCount);
    if (scrollback) {
      const lines = scrollbackLines(scrollback);
      panes.forEach((p) => p.write(lines));
      await Promise.all(panes.map((p) => p.flushed()));
    }
    await sleep(500);

    const s0 = native ? await sample(false) : null;
    let t = performance.now();
    let stop = frameClock();
    const paced = await replay(panes, { grid, visible, fromMs: panes.map((_, i) => PACED_FROM_MS + i * 400), playMs: PACED_MS }, browser);
    const pacedFrames = stop();
    const s1 = native ? await sample(true) : null;
    const pacedCpu = s0 && s1 ? cpuPercent(s0, s1, performance.now() - t) : null;

    panes.forEach((p) => p.reset());
    await sleep(300);
    const s2 = native ? await sample(false) : null;
    t = performance.now();
    stop = frameClock();
    const drain = await replay(panes, { grid, visible, fromMs: panes.map(() => 0) }, browser);
    const drainFrames = stop();
    const s3 = native ? await sample(false) : null;

    report.runs = [
      ...(report.runs as unknown[]),
      {
        panes: count,
        visible: visibleCount,
        scrollback: scrollback ?? 0,
        fontSize,
        paced: { ...paced, frameMs: pacedFrames, cpuPercent: pacedCpu },
        memory: s1,
        drain: {
          ...drain,
          recordingMbPerSec: round(((rec.bytes * count) / 1e6 / drain.ms) * 1000, 2),
          frameMs: drainFrames,
          cpuPercent: s2 && s3 ? cpuPercent(s2, s3, performance.now() - t) : null,
        },
      },
    ];
    panes.forEach((p) => p.dispose());
    await sleep(500);
  }
  return report;
}
