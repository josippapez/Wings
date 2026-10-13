import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { cn } from "@/lib/utils";

const menuOpen = () => document.querySelector('[data-slot="context-menu-content"]') !== null;

export type Side = "left" | "right";

/**
 * The state of a peek: whether the closed sidebar is showing, and the handlers that keep it showing while the
 * pointer is on the strip or the panel. Its own state, so hovering doesn't render App. `enabled` false (the sidebar
 * is docked, or there is nothing to show) drops it straight away.
 */
export function usePeek(enabled = true) {
  const [open, setOpen] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const panel = useRef<HTMLDivElement>(null);
  const later = (next: boolean, ms: number) => {
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(next), ms);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  const shown = open && enabled;
  useEffect(() => {
    if (!enabled) {
      clearTimeout(timer.current);
      setOpen(false);
    }
  }, [enabled]);

  // A click outside hides it, for when the pointer left while a context menu was open.
  useEffect(() => {
    if (!shown) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (!panel.current?.contains(target) && !target.closest('[data-slot="context-menu-content"]')) setOpen(false);
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [shown]);

  return {
    shown,
    panel,
    hide: () => {
      clearTimeout(timer.current);
      setOpen(false);
    },
    stripEvents: {
      onMouseEnter: () => later(true, 60),
      onMouseLeave: () => !open && clearTimeout(timer.current),
    },
    panelEvents: {
      onMouseEnter: () => clearTimeout(timer.current),
      onMouseLeave: () => !menuOpen() && later(false, 200),
    },
  };
}

/** The 8px hot strip at a window edge. Not in the tab order: the title bar buttons are the keyboard way in. */
export function PeekStrip({ side, events }: { side: Side; events: ReturnType<typeof usePeek>["stripEvents"] }) {
  return <div aria-hidden className={cn("absolute inset-y-0 z-30 w-2", side === "left" ? "left-0" : "right-0")} {...events} />;
}

/**
 * A closed sidebar, shown floating over the panes while the pointer is at the window's edge. It floats instead of
 * opening the panel so the terminals keep their size and don't refit. The right sidebar doesn't use this: its
 * plugin pages are frames that would reload if moved, so it floats its own element (see RightSidebar).
 */
export function SidebarPeek({ side = "left", width, children }: { side?: Side; width: number; children: ReactNode }) {
  const peek = usePeek();
  const off = side === "left" ? "-100%" : "100%";
  return (
    <>
      <PeekStrip side={side} events={peek.stripEvents} />
      <AnimatePresence>
        {peek.shown && (
          <motion.div
            ref={peek.panel}
            className={cn(
              "absolute inset-y-2 z-30 overflow-hidden border bg-popover pt-1 shadow-2xl",
              side === "left" ? "left-0 rounded-r-xl border-l-0" : "right-0 rounded-l-xl border-r-0",
            )}
            style={{ width }}
            initial={{ x: off }}
            animate={{ x: 0 }}
            exit={{ x: off }}
            transition={{ type: "spring", duration: 0.25, bounce: 0 }}
            {...peek.panelEvents}
          >
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}
