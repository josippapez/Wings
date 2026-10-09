/**
 * Benchmark-only prototype of a terminal drawn with Canvas2D from rows that Rust parsed with `alacritty_terminal`
 * (frame layout in `src-tauri/bench-grid/src/lib.rs`). No selection, input, scrollback or links: just enough to
 * show Claude Code's output correctly and measure what drawing it costs.
 */

export const FONT_FAMILY = "ui-monospace, 'SF Mono', Menlo, Monaco, Consolas, monospace";
const BACKGROUND = "#17151b";
const CURSOR = "#f0ede8";

const colors = new Map<number, string>();
const css = (rgb: number) => {
  let s = colors.get(rgb);
  if (!s) colors.set(rgb, (s = "#" + rgb.toString(16).padStart(6, "0")));
  return s;
};

export type Rect = { x: number; y: number };

/** Draws every pane with pending rows once per animation frame. */
class Scheduler {
  private pending = new Set<CanvasPane>();
  private queued = false;
  request(pane: CanvasPane) {
    this.pending.add(pane);
    if (this.queued) return;
    this.queued = true;
    requestAnimationFrame(() => {
      this.queued = false;
      const panes = [...this.pending];
      this.pending.clear();
      for (const pane of panes) pane.draw();
    });
  }
}
const scheduler = new Scheduler();
const decoder = new TextDecoder();

export class CanvasPane {
  private rows: (Uint8Array | null)[];
  private dirty = new Set<number>();
  private cursor = { row: -1, col: 0 };
  private drawnCursor = { row: -1, col: 0 };
  private ending: (() => void)[] = [];
  private readonly fonts: string[];
  readonly cellWidth: number;
  readonly cellHeight: number;

  constructor(
    private ctx: CanvasRenderingContext2D,
    private origin: Rect,
    readonly cols: number,
    rowCount: number,
    fontSize: number,
  ) {
    this.rows = Array(rowCount).fill(null);
    this.fonts = [0, 1, 2, 3].map((a) => `${a & 2 ? "italic " : ""}${a & 1 ? "bold " : ""}${fontSize}px ${FONT_FAMILY}`);
    ctx.font = this.fonts[0];
    this.cellWidth = ctx.measureText("M").width;
    this.cellHeight = Math.round(fontSize * 1.2);
  }

  /** One frame from Rust. */
  write(frame: Uint8Array) {
    const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
    const cursorRow = view.getUint16(0, true);
    this.cursor = { row: cursorRow === 0xffff ? -1 : cursorRow, col: view.getUint16(2, true) };
    let at = 6;
    for (let n = view.getUint16(4, true); n > 0; n--) {
      const start = at;
      const row = view.getUint16(at, true);
      let runs = view.getUint16(at + 2, true);
      at += 4;
      for (; runs > 0; runs--) at += 14 + view.getUint16(at + 12, true);
      this.rows[row] = frame.subarray(start, at);
      this.dirty.add(row);
    }
    scheduler.request(this);
  }

  /** Resolves once everything written so far is on the canvas. */
  flushed(): Promise<void> {
    return new Promise((done) => {
      this.ending.push(done);
      scheduler.request(this);
    });
  }

  reset() {
    this.rows.fill(null);
    this.dirty.clear();
    this.ctx.fillStyle = BACKGROUND;
    this.ctx.fillRect(this.origin.x, this.origin.y, this.cols * this.cellWidth, this.rows.length * this.cellHeight);
  }

  draw() {
    const { row, col } = this.cursor;
    const moved = row !== this.drawnCursor.row || col !== this.drawnCursor.col;
    if (moved && this.drawnCursor.row >= 0) this.dirty.add(this.drawnCursor.row);
    for (const r of this.dirty) this.drawRow(r);
    if (row >= 0 && (moved || this.dirty.has(row))) {
      this.ctx.fillStyle = CURSOR;
      this.ctx.globalAlpha = 0.6;
      this.ctx.fillRect(this.origin.x + col * this.cellWidth, this.origin.y + row * this.cellHeight, this.cellWidth, this.cellHeight);
      this.ctx.globalAlpha = 1;
    }
    this.drawnCursor = { row, col };
    this.dirty.clear();
    for (const done of this.ending.splice(0)) done();
  }

  private drawRow(r: number) {
    const bytes = this.rows[r];
    const { ctx, cellWidth: cw, cellHeight: ch } = this;
    const y = this.origin.y + r * ch;
    if (!bytes) {
      ctx.fillStyle = BACKGROUND;
      ctx.fillRect(this.origin.x, y, this.cols * cw, ch);
      return;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let at = 4;
    ctx.textBaseline = "middle";
    for (let runs = view.getUint16(2, true); runs > 0; runs--) {
      const fg = view.getUint32(at, true);
      const bg = view.getUint32(at + 4, true);
      const x = this.origin.x + view.getUint16(at + 8, true) * cw;
      const cells = view.getUint16(at + 10, true);
      const len = view.getUint16(at + 12, true);
      ctx.fillStyle = css(bg);
      ctx.fillRect(x, y, cells * cw, ch);
      if (len) {
        const attrs = fg >>> 24;
        ctx.fillStyle = css(fg & 0xffffff);
        ctx.font = this.fonts[attrs & 3];
        ctx.fillText(decoder.decode(bytes.subarray(at + 14, at + 14 + len)), x, y + ch / 2);
        if (attrs & 4) ctx.fillRect(x, y + ch - 1, cells * cw, 1);
        if (attrs & 8) ctx.fillRect(x, y + ch / 2, cells * cw, 1);
      }
      at += 14 + len;
    }
  }
}
