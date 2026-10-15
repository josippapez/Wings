import { useEffect, useRef, useState } from "react";
import { ScrollArea as ScrollAreaPrimitive } from "@base-ui/react/scroll-area";

import { ScrollBar } from "@/components/ui/scroll-area";

type Bar = { left: number; width: number; scrollWidth: number };

/** The code columns of the diff on screen: one in unified view, two in split view. Each scrolls sideways on its own. */
const columnsIn = (area: HTMLElement) => [...(area.querySelector("diffs-container")?.shadowRoot?.querySelectorAll<HTMLElement>("[data-code]") ?? [])];

/**
 * A horizontal scrollbar under each code column, pinned to the bottom of `area`. The diff library scrolls each column inside its own box, whose
 * scrollbar sits at the bottom of the file, out of view on a long one. These stand in for it and move together with the column both ways.
 * Measures at most once a frame, on a scroll or a resize; a vertical scroll can change the widest drawn line.
 */
export function DiffScrollbars({ area, watch }: { area: React.RefObject<HTMLElement | null>; watch: unknown }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const viewports = useRef<(HTMLDivElement | null)[]>([]);
  const columns = useRef<HTMLElement[]>([]);
  const syncing = useRef(false);

  useEffect(() => {
    const root = area.current;
    if (!root) return;
    let frame = 0;
    let shadow: ShadowRoot | null = null;
    const measure = () => {
      frame = 0;
      const box = root.getBoundingClientRect();
      const found = columnsIn(root);
      columns.current = found;
      const next = found.map((col) => {
        const rect = col.getBoundingClientRect();
        return { left: rect.left - box.left, width: col.clientWidth, scrollWidth: col.scrollWidth };
      });
      setBars((prev) => (prev.length === next.length && prev.every((b, i) => b.left === next[i].left && b.width === next[i].width && b.scrollWidth === next[i].scrollWidth) ? prev : next));
      // Follow a column that scrolled on its own: Shift and the wheel, or a trackpad.
      found.forEach((col, i) => {
        const vp = viewports.current[i];
        if (vp && vp.scrollLeft !== col.scrollLeft) {
          syncing.current = true;
          vp.scrollLeft = col.scrollLeft;
        }
      });
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(measure);
      // The library draws into its shadow root after a moment; listen there for the columns' own scrolls once it exists.
      const next = root.querySelector("diffs-container")?.shadowRoot ?? null;
      if (next !== shadow) {
        shadow?.removeEventListener("scroll", schedule, true);
        next?.addEventListener("scroll", schedule, { capture: true, passive: true });
        if (next) drawn.observe(next, { childList: true, subtree: true });
        shadow = next;
      }
    };
    // The diff draws after the worker highlights it, without a scroll or resize to tell us.
    const drawn = new MutationObserver(schedule);
    drawn.observe(root, { childList: true, subtree: true });
    root.addEventListener("scroll", schedule, { capture: true, passive: true });
    const resize = new ResizeObserver(schedule);
    resize.observe(root);
    schedule();
    return () => {
      cancelAnimationFrame(frame);
      root.removeEventListener("scroll", schedule, true);
      shadow?.removeEventListener("scroll", schedule, true);
      resize.disconnect();
      drawn.disconnect();
    };
  }, [area, watch]);

  return bars.map((bar, i) =>
    bar.scrollWidth > bar.width ? (
      <div key={i} className="absolute bottom-0 z-10" style={{ left: bar.left, width: bar.width }}>
      <ScrollAreaPrimitive.Root className="relative">
        <ScrollAreaPrimitive.Viewport
          ref={(el) => void (viewports.current[i] = el)}
          className="h-2.5"
          onScroll={(e) => {
            if (syncing.current) return void (syncing.current = false);
            const col = columns.current[i];
            if (col) col.scrollLeft = e.currentTarget.scrollLeft;
          }}
        >
          <div className="h-px" style={{ width: bar.scrollWidth }} />
        </ScrollAreaPrimitive.Viewport>
        <ScrollBar orientation="horizontal" />
      </ScrollAreaPrimitive.Root>
      </div>
    ) : null,
  );
}
