import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

const GAP = 6;
/** Room kept for the "+N" button when not everything fits. */
const MORE_WIDTH = 34;

/**
 * One row that shows as many items as fit its width, right-aligned, and hands the rest to `more`, so
 * plugins adding things to the same spot can't push the row out of bounds. Items are measured in an
 * invisible copy of the row, so the visible one never flickers while it settles.
 */
export function OverflowRow<T>(props: { items: T[]; keyOf: (item: T) => string; item: (item: T) => ReactNode; more: (hidden: T[]) => ReactNode }) {
  const row = useRef<HTMLDivElement>(null);
  const ruler = useRef<HTMLDivElement>(null);
  const [fits, setFits] = useState(props.items.length);
  // Re-measure when the set of items changes; size changes inside them reach the ResizeObserver.
  const keys = props.items.map(props.keyOf).join("|");

  useLayoutEffect(() => {
    const measure = () => {
      if (!row.current || !ruler.current) return;
      const widths = [...ruler.current.children].map((c) => (c as HTMLElement).offsetWidth);
      const room = row.current.clientWidth;
      const total = widths.reduce((sum, w) => sum + w, 0) + GAP * Math.max(widths.length - 1, 0);
      if (total <= room) return setFits(widths.length);
      let used = MORE_WIDTH;
      let count = 0;
      for (const w of widths) {
        if (used + w + GAP > room) break;
        used += w + GAP;
        count++;
      }
      setFits(count);
    };
    measure();
    const observer = new ResizeObserver(measure);
    if (row.current) observer.observe(row.current);
    if (ruler.current) observer.observe(ruler.current);
    return () => observer.disconnect();
  }, [keys]);

  const shown = props.items.slice(0, fits);
  const hidden = props.items.slice(fits);
  return (
    <div ref={row} className="relative flex min-w-0 flex-1 items-center justify-end gap-1.5 overflow-hidden">
      <div ref={ruler} aria-hidden inert className="pointer-events-none invisible absolute top-0 left-0 flex gap-1.5 whitespace-nowrap">
        {props.items.map((it) => (
          <div key={props.keyOf(it)} className="shrink-0">
            {props.item(it)}
          </div>
        ))}
      </div>
      {shown.map((it) => (
        <div key={props.keyOf(it)} className="shrink-0">
          {props.item(it)}
        </div>
      ))}
      {hidden.length > 0 && props.more(hidden)}
    </div>
  );
}
