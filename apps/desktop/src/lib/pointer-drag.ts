export type DragPoint = { x: number; y: number; dx: number; dy: number };

type Handlers = {
  /** The pointer moved past the threshold: it's a drag now, not a click. */
  onStart: () => void;
  onMove: (p: DragPoint) => void;
  /** `cancelled` when Escape or the system ended it. */
  onEnd: (p: DragPoint, cancelled: boolean) => void;
};

/**
 * Starts tracking a press as a possible drag. Nothing happens until the pointer moves `threshold` pixels, so a click
 * stays a click; after a drag the click that follows the release is swallowed. Listeners are on the window and move
 * handlers only touch what the caller gives them, so a drag costs no React render per frame.
 */
export function trackDrag(e: { button: number; clientX: number; clientY: number; pointerId: number }, h: Handlers, threshold = 5) {
  if (e.button !== 0) return;
  const start = { x: e.clientX, y: e.clientY };
  let dragging = false;
  const point = (ev: PointerEvent): DragPoint => ({ x: ev.clientX, y: ev.clientY, dx: ev.clientX - start.x, dy: ev.clientY - start.y });
  const finish = (p: DragPoint, cancelled: boolean) => {
    off();
    if (!dragging) return;
    // The release makes a click on the pressed element; the next click event is that one.
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    h.onEnd(p, cancelled);
  };
  const swallow = (c: Event) => c.stopPropagation();
  const move = (ev: PointerEvent) => {
    if (ev.pointerId !== e.pointerId) return;
    const p = point(ev);
    if (!dragging) {
      if (Math.hypot(p.dx, p.dy) < threshold) return;
      dragging = true;
      h.onStart();
    }
    h.onMove(p);
  };
  const up = (ev: PointerEvent) => ev.pointerId === e.pointerId && finish(point(ev), ev.type === "pointercancel");
  const key = (ev: KeyboardEvent) => {
    if (ev.key !== "Escape") return;
    ev.preventDefault();
    finish({ ...start, dx: 0, dy: 0 }, true);
  };
  const off = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
    window.removeEventListener("keydown", key, true);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
  window.addEventListener("keydown", key, true);
}
