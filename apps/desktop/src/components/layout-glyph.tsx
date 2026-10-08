import type { LayoutNode } from "@/lib/layout";

type Rect = { x: number; y: number; w: number; h: number; id: string };

const W = 16;
const H = 12;
const GAP = 1.5;

function rects(node: LayoutNode, x: number, y: number, w: number, h: number): Rect[] {
  if (node.kind === "pane") return [{ x, y, w, h, id: node.id }];
  if (node.dir === "row") {
    const a = (w - GAP) * node.ratio;
    return [...rects(node.a, x, y, a, h), ...rects(node.b, x + a + GAP, y, w - a - GAP, h)];
  }
  const a = (h - GAP) * node.ratio;
  return [...rects(node.a, x, y, w, a), ...rects(node.b, x, y + a + GAP, w, h - a - GAP)];
}

/** A tiny map of a tab's splits; the focused pane is filled. Shown only when a tab has more than one pane. */
export function LayoutGlyph({ layout, focused }: { layout: LayoutNode; focused: string }) {
  const boxes = rects(layout, 0.5, 0.5, W - 1, H - 1);
  if (boxes.length < 2) return null;
  return (
    <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} className="shrink-0" role="img" aria-label={`${boxes.length} panes`}>
      {boxes.map((r) => (
        <rect
          key={r.id}
          x={r.x}
          y={r.y}
          width={Math.max(r.w, 1)}
          height={Math.max(r.h, 1)}
          rx={1.5}
          className={r.id === focused ? "fill-current opacity-70" : "fill-none stroke-current opacity-50"}
          strokeWidth={1}
        />
      ))}
    </svg>
  );
}
