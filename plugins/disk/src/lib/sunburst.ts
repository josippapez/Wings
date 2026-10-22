// The disk map's geometry: which wedge each file and folder gets, its colour, and which wedge is under a point.
// Angles are in radians, clockwise from 12 o'clock. Ring 0 is the first ring around the centre.

import type { Entry } from "./messages";
import { childPath, find } from "./tree";

/** What a wedge stands for: a file or folder of the tree, a folder's smaller objects, or the hidden space. */
export type Target = { kind: "entry"; path: string } | { kind: "smaller"; folder: string } | { kind: "hidden" };

export interface Segment {
  target: Target;
  /** The entry it draws, for an `entry` target. */
  entry: Entry | null;
  name: string;
  size: number;
  /** What a folder holds, or what the smaller objects add up to. 0 for a file and the hidden space. */
  items: number;
  ring: number;
  a0: number;
  a1: number;
  /** A CSS colour. */
  color: string;
}

export interface Layout {
  /** By ring, and in each ring by `a0`. */
  segments: Segment[];
  /** The `entry` segments by path, for the list's colour swatches. */
  byPath: Map<string, Segment>;
  /** How many rings have segments. */
  rings: number;
}

export interface LayoutOptions {
  /** Rings to lay out around the centre. */
  rings: number;
  /** Bytes of hidden space to give a wedge in the first ring, or null. */
  hidden: number | null;
  /** Wedges narrower than this, in radians, are folded into their folder's smaller wedge. */
  minAngle: number;
}

/** Where the map is drawn: the centre, the radius of the circle in the middle, and how wide each ring is. In pixels. */
export interface Geometry {
  cx: number;
  cy: number;
  /** Where ring 0 starts. */
  inner: number;
  ringWidth: number;
}

export const TAU = Math.PI * 2;

/** The room round the map, so nothing touches the frame. */
const MARGIN = 12;
/** The circle in the middle, as a share of the map's radius. */
export const CENTER = 0.24;

/** The radius of the whole map in a `width` by `height` box. */
export const outerRadius = (width: number, height: number) => Math.max(0, Math.min(width, height) / 2 - MARGIN);

/** The map in a `width` by `height` box: square, in the middle, with `rings` rings of the same width out to the edge. */
export function geometryOf(width: number, height: number, rings: number): Geometry {
  const outer = outerRadius(width, height);
  const inner = outer * CENTER;
  return { cx: width / 2, cy: height / 2, inner, ringWidth: (outer - inner) / Math.max(1, rings) };
}

// ---------- colours ----------

const SMALLER = "Smaller objects";
const HIDDEN = "Hidden space";
/** How opaque a folder's smaller objects are, next to the folder itself. */
const SMALLER_ALPHA = 0.3;
/** How far round the hue wheel the first ring goes, in degrees. It stops short of a full turn so the first wedge and the last differ. */
const HUE_SPAN = 320;
/** How much of a wedge's own place round the wheel a folder under the first ring takes on, next to its first-ring folder's hue. */
const HUE_DRIFT = 0.3;

/** A hue round the wheel for an angle of the circle, clockwise from 12 o'clock: red, yellow, green, blue, then pink. */
const hueAt = (angle: number) => 20 + (angle / TAU) * HUE_SPAN;

const clamp = (n: number, low: number, high: number) => Math.min(Math.max(n, low), high);

/** The `rgb()` colour of OKLCH `l` (0 to 1), chroma `c` and hue `h` in degrees, cut to what a screen can show. OKLCH keeps every hue as bright as the others. */
function oklch(l: number, c: number, h: number, alpha = 1): string {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const lms = [l + 0.3963377774 * a + 0.2158037573 * b, l - 0.1055613458 * a - 0.0638541728 * b, l - 0.0894841775 * a - 1.291485548 * b].map((v) => v ** 3);
  const linear = [4.0767416621 * lms[0] - 3.3077115913 * lms[1] + 0.2309699292 * lms[2], -1.2684380046 * lms[0] + 2.6097574011 * lms[1] - 0.3413193965 * lms[2], -0.0041960863 * lms[0] - 0.7034186147 * lms[1] + 1.707614701 * lms[2]];
  const [r, g, bl] = linear.map((v) => {
    const x = clamp(v, 0, 1);
    return Math.round((x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055) * 255);
  });
  return alpha < 1 ? `rgba(${r}, ${g}, ${bl}, ${alpha})` : `rgb(${r}, ${g}, ${bl})`;
}

/** A folder of `hue` in `ring`: lighter and greyer the further out it is. */
const folderColor = (hue: number, ring: number, alpha = 1) => oklch(Math.min(0.88, 0.7 + ring * 0.034), Math.max(0.05, 0.135 - ring * 0.011), hue, alpha);
const fileColor = (ring: number) => oklch(Math.min(0.84, 0.7 + ring * 0.015), 0, 0);
const HIDDEN_COLOR = oklch(0.36, 0, 0);

// ---------- layout ----------

/** A folder whose children are about to get their wedges. */
interface Parent {
  entry: Entry;
  path: string;
  a0: number;
  a1: number;
  /** The first-ring folder it is in or is, whose hue its children keep, and the middle of its wedge. Null for the focus. */
  family: { hue: number; mid: number } | null;
  /** Its own hue, which its smaller objects take. Null for the focus. */
  hue: number | null;
}

/** Lays out the folder at `focus` in `tree` as rings around the centre. */
export function layout(tree: Entry, focus: string, { rings, hidden, minAngle }: LayoutOptions): Layout {
  const out: Layout = { segments: [], byPath: new Map(), rings: 0 };
  const trail = find(tree, focus);
  const top = trail?.[trail.length - 1];
  if (!top) return out;
  const extra = hidden !== null && hidden > 0 ? hidden : 0;
  const total = top.size + extra;
  if (!(total > 0)) return out;

  const own = TAU * (top.size / total);
  let parents: Parent[] = top.size > 0 ? [{ entry: top, path: focus, a0: 0, a1: own, family: null, hue: null }] : [];
  for (let ring = 0; ring < rings && (parents.length > 0 || (ring === 0 && extra > 0)); ring++) {
    const next: Parent[] = [];
    for (const parent of parents) place(out, parent, ring, ring + 1 < rings, minAngle, next);
    if (ring === 0 && extra > 0) {
      out.segments.push({ target: { kind: "hidden" }, entry: null, name: HIDDEN, size: extra, items: 0, ring: 0, a0: own, a1: TAU, color: HIDDEN_COLOR });
      out.rings = Math.max(out.rings, 1);
    }
    parents = next;
  }
  return out;
}

/** Gives the children of `parent` their wedges in `ring`, and the smaller objects theirs at the end. The folders to go on with are pushed to `next`. */
function place(out: Layout, parent: Parent, ring: number, deeper: boolean, minAngle: number, next: Parent[]) {
  const { entry, path, family } = parent;
  if (!(entry.size > 0)) return;
  const perByte = (parent.a1 - parent.a0) / entry.size;
  let cursor = parent.a0;
  let foldedSize = entry.smaller?.size ?? 0;
  let foldedItems = entry.smaller?.items ?? 0;

  for (const child of entry.children ?? []) {
    // A folder Wings couldn't open has no size to show.
    if (child.restricted || child.size <= 0) continue;
    const width = child.size * perByte;
    if (width < minAngle) {
      foldedSize += child.size;
      foldedItems += child.items + 1;
      continue;
    }
    const a0 = cursor;
    const a1 = cursor + width;
    cursor = a1;
    const mid = (a0 + a1) / 2;
    const childPathName = childPath(path, child.name);
    let color: string;
    let childFamily = family;
    let hue = 0;
    if (child.kind !== "dir") {
      color = fileColor(ring);
    } else if (!family) {
      // In the first ring a folder's hue comes from where it sits round the wheel.
      hue = hueAt(mid);
      childFamily = { hue, mid };
      color = folderColor(hue, ring);
    } else {
      // Deeper, it keeps its family's hue and moves along the wheel with how far it sits from the family's middle.
      hue = family.hue + ((mid - family.mid) / TAU) * HUE_SPAN * HUE_DRIFT;
      color = folderColor(hue, ring);
    }
    const segment: Segment = { target: { kind: "entry", path: childPathName }, entry: child, name: child.name, size: child.size, items: child.items, ring, a0, a1, color };
    out.segments.push(segment);
    out.byPath.set(childPathName, segment);
    out.rings = Math.max(out.rings, ring + 1);
    if (deeper && child.kind === "dir" && (child.children?.length || child.smaller)) next.push({ entry: child, path: childPathName, a0, a1, family: childFamily, hue });
  }

  if (foldedSize > 0) {
    const a1 = Math.min(parent.a1, cursor + foldedSize * perByte);
    // The focus has no colour of its own, so its smaller objects take the one a folder in their place would have.
    // They are drawn from two rings further out, because at this opacity a colour of the folder's own lightness turns to mud on a dark page.
    const hue = parent.hue ?? hueAt((cursor + a1) / 2);
    out.segments.push({ target: { kind: "smaller", folder: path }, entry: null, name: SMALLER, size: foldedSize, items: foldedItems, ring, a0: cursor, a1, color: folderColor(hue, ring + 2, SMALLER_ALPHA) });
    out.rings = Math.max(out.rings, ring + 1);
  }
}

// ---------- what is under a point ----------

const starts = new WeakMap<Layout, number[]>();

/** The index in `segments` where each ring starts, and one past the last: `[0, 12, 30]` for two rings of 12 and 18. */
function ringStarts(layout: Layout): number[] {
  let found = starts.get(layout);
  if (!found) {
    found = [];
    const { segments } = layout;
    for (let i = 0; i < segments.length; i++) {
      while (found.length <= segments[i].ring) found.push(i);
    }
    while (found.length <= layout.rings) found.push(segments.length);
    starts.set(layout, found);
  }
  return found;
}

/** The segment under the point (`x`, `y`), `"center"` when it is in the circle in the middle, or null when it is on nothing. */
export function hitTest(layout: Layout, geometry: Geometry, x: number, y: number): Segment | "center" | null {
  const dx = x - geometry.cx;
  const dy = y - geometry.cy;
  const radius = Math.hypot(dx, dy);
  if (radius < geometry.inner) return "center";
  const ring = Math.floor((radius - geometry.inner) / geometry.ringWidth);
  if (!(ring >= 0 && ring < layout.rings)) return null;

  // Clockwise from 12 o'clock, like the wedges.
  let angle = Math.atan2(dx, -dy);
  if (angle < 0) angle += TAU;
  const bounds = ringStarts(layout);
  let low = bounds[ring];
  let high = bounds[ring + 1] - 1;
  let found = -1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (layout.segments[mid].a0 <= angle) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  if (found < 0) return null;
  const segment = layout.segments[found];
  return angle < segment.a1 ? segment : null;
}
