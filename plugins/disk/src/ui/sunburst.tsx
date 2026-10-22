// The disk map: the folder at the centre, and what's in it as rings of wedges around it.

import { useLayoutEffect, useMemo, useRef, type DragEvent } from "react";

import { bytes } from "../lib/format";
import type { Entry } from "../lib/messages";
import type { Layout, Segment, Target } from "../lib/sunburst";
import { CENTER, TAU, geometryOf, hitTest, layout, outerRadius } from "../lib/sunburst";
import { find, parentPath, partsBelow } from "../lib/tree";
import "./sunburst.css";

/** The drag data a wedge or a list row carries to the Collector: a `Collected` as JSON. */
export const DRAG_TYPE = "application/x-wings-disk-entry";

export interface SunburstProps {
  /** The scan's tree. Its name is the root's full path. */
  tree: Entry;
  /** The folder at the centre: a path in the tree. */
  focus: string;
  /** Bytes of hidden space to draw as a wedge while the focus is the root of a disk scan, or null. */
  hidden: number | null;
  /** Paths in the Collector, drawn faded. */
  collected: ReadonlySet<string>;
  /** The path picked in the list, outlined. */
  selected: string | null;
  /** What the root is called in the centre while it is the focus, and in the map's label: the disk's name, like "Macintosh HD". Without it, the root's own name. */
  rootLabel?: string;
  /** The pointer is on a wedge, or on the centre (the focus folder), or left the map (null). */
  onPoint: (target: Target | null) => void;
  /** A click on a wedge. `meta` is true for a Cmd-click. */
  onClick: (target: Target, event: { meta: boolean }) => void;
  /** A click on the centre. */
  onUp: () => void;
}

/** Rings drawn at most. */
export const RINGS = 6;
/** Wedges narrower than this fold into their folder's smaller objects: 1.15 degrees, 2 pixels where the first ring starts in a map 900 pixels tall. */
export const MIN_ANGLE = 0.02;
/** How long the map takes to zoom into a folder or back out. */
const ZOOM_MS = 350;
/** How opaque a collected wedge is. */
const FADED = 0.3;
/** Half the width of the line between wedges, in pixels. */
const GAP = 0.5;

// ---------- drawing ----------

/** The colours and font of the page, which the map is drawn in. */
export interface Theme {
  surface: string;
  fg: string;
  muted: string;
  hairline: string;
  font: string;
}

/** Reads the page's colours and font from its CSS variables. */
export function readTheme(): Theme {
  const style = getComputedStyle(document.documentElement);
  const read = (name: string) => style.getPropertyValue(name).trim();
  return { surface: read("--wings-surface"), fg: read("--wings-fg"), muted: read("--wings-muted"), hairline: read("--wings-hairline"), font: read("--wings-font") };
}

/** How the map is zooming between two views of the tree, one of them centred on a folder in the other. */
interface Zoom {
  /** The wedge the map zooms into, as it is in the zoomed-out view: where it starts, and how wide it is. */
  from: number;
  span: number;
  /** How many rings further out the zoomed-in view's first ring is in the zoomed-out view. */
  depth: number;
  /** 0 is the zoomed-out view, 1 the zoomed-in one. */
  u: number;
  /** How many rings each view has, to tell the wedges that are in only one of them. */
  outRings: number;
  inRings: number;
}

interface Layer {
  layout: Layout;
  /** The wedges drawn faded because they are in the Collector. */
  faded: ReadonlySet<Segment>;
  /** How opaque the whole layer is. */
  alpha: number;
  zoom: Zoom | null;
  /** The layer is the zoomed-in view, whose angles start at its own 12 o'clock rather than in the zoomed-out view's. */
  nested: boolean;
}

/** The lines of text in the middle: a title, and the first of the details that fits. */
export interface Words {
  title: string;
  details: string[];
}

export interface Scene {
  /** The size of the canvas in CSS pixels. */
  width: number;
  height: number;
  theme: Theme;
  layers: Layer[];
  /** How wide a ring is, in pixels. */
  ringWidth: number;
  hover: Segment | "center" | null;
  selected: Segment | null;
  words: Words;
  /** The pointer is on the centre and a click would go up. */
  up: boolean;
}

type Ctx = CanvasRenderingContext2D;
const HALF_PI = Math.PI / 2;

/**
 * Adds a wedge to the path: from `r0` to `r1` out from the middle, from `a0` to `a1` clockwise from 12 o'clock, pulled in by
 * `inset` pixels on every side unless that would leave nothing of it.
 */
function wedge(ctx: Ctx, cx: number, cy: number, r0: number, r1: number, a0: number, a1: number, inset: number) {
  const d = r1 - r0 > 2 * inset && (a1 - a0) * r0 > 2 * inset ? inset : 0;
  const start = a0 - HALF_PI;
  const end = a1 - HALF_PI;
  const near = d / r0;
  const far = d / r1;
  ctx.moveTo(cx + (r1 - d) * Math.cos(start + far), cy + (r1 - d) * Math.sin(start + far));
  ctx.arc(cx, cy, r1 - d, start + far, end - far);
  ctx.arc(cx, cy, r0 + d, end - near, start + near, true);
  ctx.closePath();
}

/** Where `angle` of the zoomed-out view is `u` of the way to its place in the zoomed-in view, where the wedge zoomed into is the whole circle. */
const warp = (zoom: Zoom, angle: number) => angle + (Math.min(Math.max((angle - zoom.from) / zoom.span, 0), 1) * TAU - angle) * zoom.u;

const clamp = (n: number, low: number, high: number) => Math.min(Math.max(n, low), high);

/** Draws a layer's wedges, each pulled in by half a pixel, which leaves a line the colour of the page between them. */
function paintLayer(ctx: Ctx, layer: Layer, cx: number, cy: number, inner: number, outer: number, ringWidth: number) {
  const { faded, zoom, nested } = layer;
  if (layer.alpha <= 0.005) return;
  let color = "";
  for (const s of layer.layout.segments) {
    let ring = s.ring;
    let a0 = s.a0;
    let a1 = s.a1;
    let alpha = layer.alpha;
    if (zoom && nested) {
      // The zoomed-in view, put where its wedges are in the zoomed-out one, and moved with them.
      const scale = zoom.span / TAU;
      ring += zoom.depth * (1 - zoom.u);
      a0 = warp(zoom, zoom.from + a0 * scale);
      a1 = warp(zoom, zoom.from + a1 * scale);
    } else if (zoom) {
      // A wedge that is in only one of the views fades out of it, or into it, as the map moves.
      const before = ring < zoom.outRings ? 1 : 0;
      const after = ring >= zoom.depth && ring < zoom.depth + zoom.inRings && a1 > zoom.from && a0 < zoom.from + zoom.span ? 1 : 0;
      alpha *= before + (after - before) * zoom.u;
      ring -= zoom.depth * zoom.u;
      a0 = warp(zoom, a0);
      a1 = warp(zoom, a1);
    }
    if (faded.has(s)) alpha *= FADED;
    const r0 = clamp(inner + ring * ringWidth, inner, outer);
    const r1 = clamp(inner + (ring + 1) * ringWidth, inner, outer);
    // Too small to see.
    if (alpha <= 0.01 || r1 - r0 < 0.5 || (a1 - a0) * r1 < 0.5) continue;
    if (s.color !== color) ctx.fillStyle = color = s.color;
    ctx.globalAlpha = alpha;
    ctx.beginPath();
    wedge(ctx, cx, cy, r0, r1, a0, a1, GAP);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

/** `text`, cut short with an ellipsis until it is no wider than `max`. */
function fit(ctx: Ctx, text: string, max: number): string {
  if (ctx.measureText(text).width <= max) return text;
  let low = 0;
  let high = text.length - 1;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (ctx.measureText(`${text.slice(0, mid)}…`).width <= max) low = mid;
    else high = mid - 1;
  }
  return `${text.slice(0, low).trimEnd()}…`;
}

/** Draws one frame of the map. */
export function paint(ctx: Ctx, scene: Scene) {
  const { width, height, theme, ringWidth } = scene;
  ctx.clearRect(0, 0, width, height);
  const outer = outerRadius(width, height);
  if (outer < 8) return;
  const cx = width / 2;
  const cy = height / 2;
  const inner = outer * CENTER;
  for (const layer of scene.layers) paintLayer(ctx, layer, cx, cy, inner, outer, ringWidth);

  // What is picked, outlined, and what the pointer is on, brighter.
  if (scene.selected) {
    const s = scene.selected;
    ctx.beginPath();
    wedge(ctx, cx, cy, inner + s.ring * ringWidth, inner + (s.ring + 1) * ringWidth, s.a0, s.a1, 1);
    ctx.lineWidth = 2;
    ctx.lineJoin = "round";
    ctx.strokeStyle = theme.fg;
    ctx.stroke();
  }
  if (scene.hover && scene.hover !== "center") {
    const s = scene.hover;
    ctx.beginPath();
    wedge(ctx, cx, cy, inner + s.ring * ringWidth, inner + (s.ring + 1) * ringWidth, s.a0, s.a1, GAP);
    ctx.globalAlpha = 0.32;
    ctx.fillStyle = theme.fg;
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  // The middle.
  ctx.beginPath();
  ctx.arc(cx, cy, inner - 1, 0, TAU);
  ctx.fillStyle = theme.surface;
  ctx.fill();
  if (scene.up) {
    ctx.globalAlpha = 0.1;
    ctx.fillStyle = theme.fg;
    ctx.fill();
    ctx.globalAlpha = 1;
  }
  ctx.lineWidth = 1;
  ctx.strokeStyle = theme.hairline;
  ctx.stroke();

  const size = clamp(Math.round(inner * 0.16), 13, 19);
  const small = Math.max(11, Math.round(size * 0.82));
  const gap = size * 0.78;
  // What a line can use: the circle's chord at the line's edge farthest from the middle, less some room.
  const roomAt = (offset: number) => 2 * Math.sqrt(Math.max(0, inner * inner - offset * offset)) - 12;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  // A small map shrinks a long title, down to 11px, before it cuts it short.
  let titleSize = size;
  ctx.font = `600 ${titleSize}px ${theme.font}`;
  while (titleSize > 11 && ctx.measureText(scene.words.title).width > roomAt(gap + titleSize / 2)) ctx.font = `600 ${--titleSize}px ${theme.font}`;
  ctx.fillStyle = theme.fg;
  ctx.fillText(fit(ctx, scene.words.title, roomAt(gap + titleSize / 2)), cx, cy - gap);
  ctx.font = `${small}px ${theme.font}`;
  ctx.fillStyle = theme.muted;
  const room = roomAt(gap + small / 2);
  const detail = scene.words.details.find((d) => ctx.measureText(d).width <= room) ?? scene.words.details[scene.words.details.length - 1];
  if (detail !== undefined) ctx.fillText(fit(ctx, detail, room), cx, cy + gap);
}

// ---------- the map ----------

interface Model {
  tree: Entry;
  focus: string;
  hidden: number | null;
  collected: ReadonlySet<string>;
  selected: string | null;
  rootLabel: string | undefined;
}

type Handlers = Pick<SunburstProps, "onPoint" | "onClick" | "onUp">;

/** A move between two views: `big` is the zoomed-out one with rings to spare, and `over` the zoomed-in one. */
interface Move {
  start: number;
  /** True when the map zooms in, false when it zooms back out. */
  forward: boolean;
  big: Layout;
  over: Layout;
  zoom: Omit<Zoom, "u">;
}

const easeInOut = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);
const smoothstep = (low: number, high: number, x: number) => {
  const t = clamp((x - low) / (high - low), 0, 1);
  return t * t * (3 - 2 * t);
};

const nameOf = (path: string) => (path === "/" ? "/" : path.slice(path.lastIndexOf("/") + 1) || path);
/** What the folder at `focus` is called: the page's label for the root of the tree, else its own name. */
const titleOf = (tree: Entry, focus: string, rootLabel: string | undefined) => (focus === tree.name && rootLabel) || nameOf(focus);
const itemsOf = (n: number) => `${n.toLocaleString()} ${n === 1 ? "item" : "items"}`;
const targetKey = (t: Target) => (t.kind === "entry" ? `entry ${t.path}` : t.kind === "smaller" ? `smaller ${t.folder}` : "hidden");

/** The wedges in the Collector, and the ones inside a folder that is. */
const fadedCache = new WeakMap<Layout, { collected: ReadonlySet<string>; faded: ReadonlySet<Segment> }>();
const NONE: ReadonlySet<Segment> = new Set();

function fadedIn(layout: Layout, collected: ReadonlySet<string>): ReadonlySet<Segment> {
  if (collected.size === 0) return NONE;
  const known = fadedCache.get(layout);
  if (known?.collected === collected) return known.faded;
  const below = new Map<string, boolean>();
  const inside = (path: string | null): boolean => {
    if (path === null) return false;
    let found = below.get(path);
    if (found === undefined) {
      found = collected.has(path) || inside(parentPath(path));
      below.set(path, found);
    }
    return found;
  };
  const faded = new Set<Segment>();
  for (const s of layout.segments) {
    if (s.target.kind === "hidden") continue;
    if (inside(s.target.kind === "entry" ? s.target.path : s.target.folder)) faded.add(s);
  }
  fadedCache.set(layout, { collected, faded });
  return faded;
}

/** Everything the canvas does that React doesn't need to know about: sizing, drawing, the pointer and the zoom. */
class MapView {
  handlers: Handlers;
  private readonly ctx: Ctx;
  private model: Model | null = null;
  private focused: Entry | null = null;
  private current: Layout | null = null;
  private readonly layouts = new Map<string, Layout>();
  private move: Move | null = null;
  private hover: Segment | "center" | null = null;
  /** The key of the target `onPoint` was last told about. */
  private told: string | null = null;
  private pointer: { x: number; y: number } | null = null;
  /** Read when the first frame is drawn, and again after the colour scheme changes. */
  private theme: Theme | null = null;
  private size: { width: number; height: number };
  private frame = 0;
  private readonly observer: ResizeObserver;
  private readonly colorScheme = matchMedia("(prefers-color-scheme: dark)");

  constructor(
    private readonly canvas: HTMLCanvasElement,
    host: HTMLElement,
    handlers: Handlers,
  ) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("This page can't draw on a canvas.");
    this.ctx = ctx;
    this.handlers = handlers;
    this.size = { width: host.clientWidth, height: host.clientHeight };
    // The observer only notes the size. Drawing waits for the next frame, so a size that changes every frame is drawn once each.
    this.observer = new ResizeObserver(([entry]) => {
      this.size = { width: entry.contentRect.width, height: entry.contentRect.height };
      this.schedule();
    });
    this.observer.observe(host);
    this.colorScheme.addEventListener("change", this.onColorScheme);
  }

  destroy() {
    cancelAnimationFrame(this.frame);
    this.observer.disconnect();
    this.colorScheme.removeEventListener("change", this.onColorScheme);
  }

  private readonly onColorScheme = () => {
    this.theme = null;
    this.schedule();
  };

  private schedule() {
    if (!this.frame) this.frame = requestAnimationFrame(this.onFrame);
  }

  /** The layout of `focus` with `rings` rings, kept for the next time. */
  private layoutFor(focus: string, rings: number): Layout {
    const { tree, hidden } = this.model!;
    const key = `${rings} ${focus}`;
    let found = this.layouts.get(key);
    if (found) {
      this.layouts.delete(key);
    } else {
      found = layout(tree, focus, { rings, hidden: focus === tree.name ? hidden : null, minAngle: MIN_ANGLE });
      if (this.layouts.size >= 8) this.layouts.delete(this.layouts.keys().next().value!);
    }
    this.layouts.set(key, found);
    return found;
  }

  update(model: Model) {
    const before = this.model;
    this.model = model;
    const sameTree = before?.tree === model.tree && before.hidden === model.hidden;
    if (!sameTree) this.layouts.clear();
    // What is drawn changes with the tree and the focus. Another selection or Collector only changes how it is drawn, and doesn't end a zoom.
    if (!before || !sameTree || before.focus !== model.focus) {
      const previous = this.current;
      this.current = this.layoutFor(model.focus, RINGS);
      this.focused = find(model.tree, model.focus)?.at(-1) ?? null;
      this.move = null;
      if (previous && sameTree && before) this.startMove(before.focus, model.focus);
      this.rehover();
    }
    this.schedule();
  }

  /** Starts the map zooming from one focus to the other, when one is a folder in the other and the pointer isn't asking for less motion. */
  private startMove(from: string, to: string) {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const into = partsBelow(from, to);
    const depth = (into ?? partsBelow(to, from))?.length ?? 0;
    if (depth < 1 || depth > RINGS) return;
    // The zoomed-out view's folder is the outer one, with rings to spare for what the zoomed-in view shows.
    const outer = into ? from : to;
    const inner = into ? to : from;
    const big = this.layoutFor(outer, RINGS + depth);
    const target = big.byPath.get(inner);
    if (!target) return;
    const out = this.layoutFor(outer, RINGS);
    const over = this.layoutFor(inner, RINGS);
    this.move = {
      start: performance.now(),
      forward: Boolean(into),
      big,
      over,
      zoom: { from: target.a0, span: target.a1 - target.a0, depth, outRings: out.rings, inRings: over.rings },
    };
  }

  // ---------- drawing ----------

  private readonly onFrame = (now: number) => {
    this.frame = 0;
    if (this.move && now - this.move.start >= ZOOM_MS) {
      this.move = null;
      // What is under the pointer is another wedge now.
      this.rehover();
    }
    this.draw(now);
    if (this.move) this.schedule();
  };

  private draw(now: number) {
    const { width, height } = this.size;
    if (!(width > 0 && height > 0) || !this.model || !this.current) return;
    const ratio = window.devicePixelRatio || 1;
    const w = Math.round(width * ratio);
    const h = Math.round(height * ratio);
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    this.ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    paint(this.ctx, this.scene(now));
  }

  private scene(now: number): Scene {
    const { width, height } = this.size;
    const model = this.model!;
    const current = this.current!;
    const outer = outerRadius(width, height);
    const inner = outer * CENTER;
    const move = this.move;
    const base = { width, height, theme: (this.theme ??= readTheme()), words: this.words(), up: this.hover === "center" && this.canGoUp() };
    if (!move) {
      const selected = model.selected === null ? null : (current.byPath.get(model.selected) ?? null);
      return { ...base, layers: [{ layout: current, faded: fadedIn(current, model.collected), alpha: 1, zoom: null, nested: false }], ringWidth: (outer - inner) / Math.max(1, current.rings), hover: this.hover, selected };
    }
    const eased = easeInOut(clamp((now - move.start) / ZOOM_MS, 0, 1));
    const u = move.forward ? eased : 1 - eased;
    const zoom = { ...move.zoom, u };
    // Rings are as wide as the views' are, and the width moves from one to the other.
    const from = (outer - inner) / Math.max(1, zoom.outRings);
    const to = (outer - inner) / Math.max(1, zoom.inRings);
    // Near the end the zoomed-in view fades in over the zoomed-out one: its folders have their own colours and more of the small ones show.
    const into = smoothstep(0.35, 1, u);
    return {
      ...base,
      layers: [
        { layout: move.big, faded: fadedIn(move.big, model.collected), alpha: 1 - into, zoom, nested: false },
        { layout: move.over, faded: fadedIn(move.over, model.collected), alpha: into, zoom, nested: true },
      ],
      ringWidth: from + (to - from) * u,
      hover: null,
      selected: null,
    };
  }

  private canGoUp() {
    return this.model !== null && this.model.focus !== this.model.tree.name;
  }

  /** What the middle says: what the pointer is on, or else the folder at the centre. */
  private words(): Words {
    const model = this.model!;
    const hover = this.hover;
    const details = (size: number, items: number | null) => (items === null ? [bytes(size)] : [`${bytes(size)} · ${itemsOf(items)}`, bytes(size)]);
    if (hover && hover !== "center") {
      if (hover.target.kind === "hidden") return { title: hover.name, details: details(hover.size, null) };
      return { title: hover.name, details: details(hover.size, hover.target.kind === "smaller" || hover.entry?.kind === "dir" ? hover.items : null) };
    }
    const up = hover === "center" && this.canGoUp() ? parentPath(model.focus) : null;
    if (up !== null) return { title: `Up to ${titleOf(model.tree, up, model.rootLabel)}`, details: ["Click to go up"] };
    return { title: titleOf(model.tree, model.focus, model.rootLabel), details: this.focused ? details(this.focused.size, this.focused.items) : [] };
  }

  // ---------- the pointer ----------

  private geometry() {
    return geometryOf(this.size.width, this.size.height, this.current?.rings ?? 1);
  }

  private hit(x: number, y: number) {
    return this.current && !this.move ? hitTest(this.current, this.geometry(), x, y) : null;
  }

  private setHover(hit: Segment | "center" | null) {
    if (hit !== this.hover) {
      this.hover = hit;
      this.canvas.style.cursor = hit !== null && (hit !== "center" || this.canGoUp()) ? "pointer" : "";
      this.schedule();
    }
    // The centre stands for the folder at the focus.
    const target: Target | null = hit === null ? null : hit === "center" ? { kind: "entry", path: this.model!.focus } : hit.target;
    const key = target && targetKey(target);
    if (key !== this.told) {
      this.told = key;
      this.handlers.onPoint(target);
    }
  }

  /** Looks again at what is under the pointer, after the map changed under it. */
  private rehover() {
    if (this.move) {
      this.hover = null;
      return;
    }
    this.setHover(this.pointer ? this.hit(this.pointer.x, this.pointer.y) : null);
  }

  pointTo(x: number, y: number) {
    this.pointer = { x, y };
    if (!this.move) this.setHover(this.hit(x, y));
  }

  leave() {
    this.pointer = null;
    this.setHover(null);
  }

  click(x: number, y: number, meta: boolean) {
    const hit = this.hit(x, y);
    if (hit === "center") this.handlers.onUp();
    else if (hit) this.handlers.onClick(hit.target, { meta });
  }

  /** Starts dragging the file or folder under the pointer to the Collector, and refuses to drag anything else. */
  dragStart(event: DragEvent<HTMLCanvasElement>) {
    const hit = this.hit(event.nativeEvent.offsetX, event.nativeEvent.offsetY);
    if (hit === null || hit === "center" || hit.target.kind !== "entry" || !hit.entry) return event.preventDefault();
    event.dataTransfer.setData(DRAG_TYPE, JSON.stringify({ path: hit.target.path, size: hit.size, kind: hit.entry.kind }));
    event.dataTransfer.effectAllowed = "copy";
    // The default drag image is the whole canvas. A label with the name and size goes in the page for the moment the drag takes its picture.
    const label = document.createElement("div");
    label.className = "sunburst-drag";
    const name = document.createElement("strong");
    name.textContent = hit.name;
    const size = document.createElement("span");
    size.textContent = bytes(hit.size);
    label.append(name, size);
    document.body.append(label);
    event.dataTransfer.setDragImage(label, 12, 14);
    setTimeout(() => label.remove());
  }
}

export function Sunburst(props: SunburstProps) {
  const { tree, focus, hidden, collected, selected, rootLabel, onPoint, onClick, onUp } = props;
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef<MapView | null>(null);

  useLayoutEffect(() => {
    const map = new MapView(canvas.current!, host.current!, { onPoint, onClick, onUp });
    view.current = map;
    return () => {
      map.destroy();
      view.current = null;
    };
    // Made once. The handlers and the model reach it below.
  }, []);
  useLayoutEffect(() => {
    if (view.current) view.current.handlers = { onPoint, onClick, onUp };
  });
  useLayoutEffect(() => {
    view.current?.update({ tree, focus, hidden, collected, selected, rootLabel });
  }, [tree, focus, hidden, collected, selected, rootLabel]);

  const label = useMemo(() => {
    const entry = find(tree, focus)?.at(-1);
    return `Disk map of ${titleOf(tree, focus, rootLabel)}${entry ? `, ${bytes(entry.size)}` : ""}`;
  }, [tree, focus, rootLabel]);

  return (
    <div className="sunburst" ref={host}>
      <canvas
        ref={canvas}
        role="img"
        aria-label={label}
        draggable
        onPointerMove={(e) => view.current?.pointTo(e.nativeEvent.offsetX, e.nativeEvent.offsetY)}
        onPointerLeave={() => view.current?.leave()}
        onClick={(e) => view.current?.click(e.nativeEvent.offsetX, e.nativeEvent.offsetY, e.metaKey)}
        onDragStart={(e) => view.current?.dragStart(e)}
        onDragEnd={() => view.current?.leave()}
      />
    </div>
  );
}
