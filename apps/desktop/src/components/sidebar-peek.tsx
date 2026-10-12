import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState, type ReactNode } from "react";

const menuOpen = () => document.querySelector('[data-slot="context-menu-content"]') !== null;

/**
 * The closed sidebar, shown floating over the panes while the pointer is at the window's left edge. It floats
 * instead of opening the panel so the terminals keep their size and don't refit. Its own state, so hovering
 * doesn't render App.
 */
export function SidebarPeek({ width, children }: { width: number; children: ReactNode }) {
  const [shown, setShown] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const panel = useRef<HTMLDivElement>(null);
  const later = (next: boolean, ms: number) => {
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setShown(next), ms);
  };
  useEffect(() => () => clearTimeout(timer.current), []);

  // A click outside hides it, for when the pointer left while a context menu was open.
  useEffect(() => {
    if (!shown) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (!panel.current?.contains(target) && !target.closest('[data-slot="context-menu-content"]')) setShown(false);
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [shown]);

  return (
    <>
      <div aria-hidden className="absolute inset-y-0 left-0 z-30 w-1.5" onMouseEnter={() => later(true, 60)} onMouseLeave={() => !shown && clearTimeout(timer.current)} />
      <AnimatePresence>
        {shown && (
          <motion.div
            ref={panel}
            className="absolute inset-y-2 left-0 z-30 overflow-hidden rounded-r-xl border border-l-0 bg-popover pt-1 shadow-2xl"
            style={{ width }}
            initial={{ x: "-100%" }}
            animate={{ x: 0 }}
            exit={{ x: "-100%" }}
            transition={{ type: "spring", duration: 0.25, bounce: 0 }}
            onMouseEnter={() => clearTimeout(timer.current)}
            onMouseLeave={() => !menuOpen() && later(false, 200)}
          >
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}
