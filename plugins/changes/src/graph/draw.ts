import { MAX_LANES, type Commit } from "./layout";

export const ROW = 26;
const LANE = 14;

const css = getComputedStyle(document.documentElement);
/** Wings' own colours first, then four more that read on the dark background. */
const PALETTE = [...["--wings-info", "--wings-success", "--wings-warning", "--wings-danger"].map((v) => css.getPropertyValue(v).trim()), "#c4a7ff", "#6fd3d0", "#f0a3d0", "#b9cf6e"];

export const laneX = (lane: number) => 16 + lane * LANE;
export const graphWidth = (lanes: number) => laneX(lanes) + 4;

/** The lanes of rows `top` to `top + size`, onto a canvas as tall as those rows. */
export function draw(canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D, commits: Commit[], lanes: number, selected: number, top: number, size: number) {
  const dpr = window.devicePixelRatio || 1;
  const width = graphWidth(lanes);
  const height = size * ROW;
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.lineWidth = 1.75;
  ctx.lineCap = "round";
  for (let k = 0; k < size; k++) {
    const c = commits[top + k];
    const y0 = k * ROW;
    const ym = y0 + ROW / 2;
    const y1 = y0 + ROW;
    const l = c.lines;
    for (let n = 0; n < l.length; n += 4) {
      const [a, b, part] = [l[n], l[n + 1], l[n + 2]];
      if (a >= MAX_LANES && b >= MAX_LANES) continue;
      const [xa, xb] = [laneX(a), laneX(b)];
      const [ya, yb] = part === 0 ? [y0, ym] : part === 1 ? [ym, y1] : [y0, y1];
      ctx.strokeStyle = PALETTE[l[n + 3] % PALETTE.length];
      ctx.beginPath();
      ctx.moveTo(xa, ya);
      if (xa === xb) ctx.lineTo(xb, yb);
      else ctx.bezierCurveTo(xa, (ya + yb) / 2, xb, (ya + yb) / 2, xb, yb);
      ctx.stroke();
    }
  }
  for (let k = 0; k < size; k++) {
    const c = commits[top + k];
    if (c.lane >= MAX_LANES) continue;
    const x = laneX(c.lane);
    const y = k * ROW + ROW / 2;
    ctx.fillStyle = PALETTE[c.color % PALETTE.length];
    ctx.beginPath();
    ctx.arc(x, y, top + k === selected ? 5 : 4, 0, Math.PI * 2);
    ctx.fill();
    if (c.parents.length > 1) {
      // A merge is a ring, so it stands out from the commits on its lane.
      ctx.globalCompositeOperation = "destination-out";
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = "source-over";
    }
  }
}
