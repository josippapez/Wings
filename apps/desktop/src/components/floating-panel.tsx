import { useRef } from "react";

import { trackDrag } from "@/lib/pointer-drag";
import { cn } from "@/lib/utils";

/** Where the panel sits, as distances from the bottom-right corner of its area, so a smaller window never leaves it out of view. */
export type Rect = { right: number; bottom: number; w: number; h: number };

const MIN_W = 280;
const MIN_H = 120;
export const PANEL_EDGE = 12;

/**
 * The frame of a terminal panel floated over an area: drag the header to move it, the corner to resize it, and it
 * shrinks to a bar when `collapsed`. Moves and resizes touch only the panel's own style, so a terminal inside never
 * reflows during a gesture; `onRect` runs once, when the gesture ends.
 */
export function FloatingPanel({ rect, collapsed, onRect, onExpand, label, overlayClassName, header, children }: {
  rect: Rect;
  collapsed: boolean;
  onRect: (rect: Rect) => void;
  /** A click on the collapsed bar. */
  onExpand: () => void;
  label: string;
  overlayClassName?: string;
  /** What the header row holds. Anything marked `data-no-drag` takes its own clicks instead of starting a move. */
  header: React.ReactNode;
  /** Kept mounted while collapsed, hidden, so a terminal's scrollback and shell are untouched. */
  children: React.ReactNode;
}) {
  const area = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  const move = (e: React.PointerEvent) => {
    const el = panel.current;
    const box = area.current;
    if (!el || !box || (e.target as HTMLElement).closest("[data-no-drag]")) return;
    let next = rect;
    trackDrag(e, {
      onStart: () => (el.style.willChange = "right, bottom"),
      onMove: ({ dx, dy }) => {
        const w = el.offsetWidth;
        const h = el.offsetHeight;
        next = { ...rect, right: Math.min(Math.max(rect.right - dx, 0), box.clientWidth - w), bottom: Math.min(Math.max(rect.bottom - dy, 0), box.clientHeight - h) };
        el.style.right = `${next.right}px`;
        el.style.bottom = `${next.bottom}px`;
      },
      onEnd: (_, cancelled) => {
        el.style.willChange = "";
        if (cancelled) {
          el.style.right = `${rect.right}px`;
          el.style.bottom = `${rect.bottom}px`;
        } else onRect(next);
      },
    });
  };

  const resize = (e: React.PointerEvent) => {
    const el = panel.current;
    const box = area.current;
    if (!el || !box) return;
    e.stopPropagation();
    let next = rect;
    trackDrag(e, {
      onStart: () => (el.style.willChange = "width, height"),
      onMove: ({ dx, dy }) => {
        next = { ...rect, w: Math.min(Math.max(rect.w - dx, MIN_W), box.clientWidth - rect.right), h: Math.min(Math.max(rect.h - dy, MIN_H), box.clientHeight - rect.bottom) };
        el.style.width = `${next.w}px`;
        el.style.height = `${next.h}px`;
      },
      onEnd: (_, cancelled) => {
        el.style.willChange = "";
        if (cancelled) {
          el.style.width = `${rect.w}px`;
          el.style.height = `${rect.h}px`;
        } else onRect(next);
      },
    }, 2);
  };

  return (
    <div
      ref={area}
      // Clicks pass through the overlay to what is under it; only the panel takes them, and a press on it keeps its own focus.
      onMouseDown={(e) => e.stopPropagation()}
      className={cn("pointer-events-none absolute inset-0", overlayClassName ?? "z-20")}
    >
      <div
        ref={panel}
        role="group"
        aria-label={label}
        style={{ right: rect.right, bottom: rect.bottom, ...(collapsed ? {} : { width: rect.w, height: rect.h }) }}
        className={cn(
          "pointer-events-auto absolute flex max-h-full max-w-full flex-col overflow-hidden border border-hairline-strong bg-surface text-[12px] shadow-lg",
          collapsed ? "h-7 rounded-full bg-popover/90 backdrop-blur-xl" : "rounded-xl",
        )}
      >
        <div
          onPointerDown={move}
          onClick={collapsed ? onExpand : undefined}
          className={cn("flex shrink-0 cursor-grab items-center gap-1.5 select-none active:cursor-grabbing", collapsed ? "h-7 pr-1 pl-3" : "h-8 pr-1 pl-3")}
        >
          {header}
        </div>
        <div className={cn("flex min-h-0 flex-1 flex-col", collapsed && "hidden")}>{children}</div>
        {!collapsed && <div onPointerDown={resize} aria-hidden className="absolute top-0 left-0 size-4 cursor-nwse-resize touch-none" />}
      </div>
    </div>
  );
}
