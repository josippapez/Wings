import { useRef, useState, useSyncExternalStore } from "react";
import { flushSync } from "react-dom";
import { motion } from "motion/react";
import { CheckIcon, MinusIcon, XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { CORNERS, nearestCorner, railProgress, type Corner, type PaneRail } from "@/lib/pane-rail";
import { trackDrag } from "@/lib/pointer-drag";
import { cn } from "@/lib/utils";

export type PaneRailEntry = { pluginId: string; rail: PaneRail };

// The corner and whether the card is minimised are one setting for every pane, kept in localStorage like the other UI settings.
const CORNER_KEY = "pane-rail-corner";
const MINIMISED_KEY = "pane-rail-minimised";
const listeners = new Set<() => void>();
const subscribe = (cb: () => void) => (listeners.add(cb), () => void listeners.delete(cb));
const read = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const write = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Unwritable storage: the setting lasts until Wings closes, which the listeners below still give.
  }
  memory.set(key, value);
  listeners.forEach((cb) => cb());
};
const memory = new Map<string, string>();
const get = (key: string) => memory.get(key) ?? read(key);

/** What closing a rail hid: its JSON, by pane and plugin. The same rail stays hidden; a different one brings it back. */
const dismissed = new Map<string, string>();

const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

function Marker({ status }: { status: PaneRail["items"][number]["status"] }) {
  if (status === "done") {
    return (
      <span className="mt-px flex size-3.5 shrink-0 items-center justify-center rounded-full bg-white/10 text-muted-foreground" aria-hidden>
        <CheckIcon className="size-2.5" strokeWidth={3} />
      </span>
    );
  }
  return (
    <span className={cn("mt-px flex size-3.5 shrink-0 items-center justify-center rounded-full border", status === "in_progress" ? "border-foreground" : "border-white/25")} aria-hidden>
      {status === "in_progress" && <span className="size-1.5 rounded-full bg-foreground" />}
    </span>
  );
}

const statusLabel = { done: "Done", in_progress: "In progress", pending: "Pending" };

function Card({ rail, minimised, onMinimise, onClose, onOpen, onGrab }: {
  rail: PaneRail;
  minimised: boolean;
  onMinimise: () => void;
  onClose: () => void;
  onOpen?: () => void;
  onGrab: (e: React.PointerEvent) => void;
}) {
  const progress = railProgress(rail);
  const close = (
    <Button variant="ghost" size="icon-xs" data-no-drag aria-label={`Close ${rail.title}`} title="Close, until it changes" onClick={onClose} className="text-muted-foreground">
      <XIcon />
    </Button>
  );
  if (minimised) {
    return (
      <div className="pointer-events-auto flex h-7 max-w-full items-center rounded-full border border-hairline-strong bg-popover/90 pr-0.5 text-[12px] shadow-lg backdrop-blur-xl">
        <button
          type="button"
          onPointerDown={onGrab}
          onClick={onMinimise}
          aria-label={`Expand ${rail.title}`}
          className="flex h-full min-w-0 cursor-grab items-center gap-1.5 rounded-full pr-1.5 pl-3 outline-none select-none focus-visible:ring-1 focus-visible:ring-ring active:cursor-grabbing"
        >
          <span className="truncate">{rail.title}</span>
          <span className="text-muted-foreground tabular-nums">· {progress}</span>
        </button>
        {close}
      </div>
    );
  }
  return (
    <div role="group" aria-label={rail.title} className="pointer-events-auto flex max-h-full w-64 max-w-full flex-col overflow-hidden rounded-xl border border-hairline-strong bg-popover/90 text-[12px] shadow-lg backdrop-blur-xl">
      <div onPointerDown={onGrab} className="flex h-8 shrink-0 cursor-grab items-center gap-1 pr-1 pl-3 select-none active:cursor-grabbing">
        {onOpen ? (
          <button type="button" data-no-drag onClick={onOpen} className="min-w-0 truncate rounded-sm font-medium outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring">
            {rail.title}
          </button>
        ) : (
          <span className="min-w-0 truncate font-medium">{rail.title}</span>
        )}
        <span className="text-muted-foreground tabular-nums">{progress}</span>
        <span className="flex-1" />
        <Button variant="ghost" size="icon-xs" data-no-drag aria-label={`Minimise ${rail.title}`} title="Minimise" onClick={onMinimise} className="text-muted-foreground">
          <MinusIcon />
        </Button>
        {close}
      </div>
      {rail.items.length > 0 && (
        <ul className="min-h-0 flex-1 space-y-1.5 overflow-y-auto px-3 pt-0.5 pb-2.5">
          {rail.items.map((item, i) => (
            <li key={i} className="flex items-start gap-2 leading-snug">
              <Marker status={item.status} />
              <span className={cn("min-w-0 break-words", item.status === "done" ? "text-muted-foreground/60 line-through" : item.status === "in_progress" ? "font-medium text-foreground" : "text-foreground/80")}>
                <span className="sr-only">{statusLabel[item.status]}: </span>
                {item.text}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * The cards plugins float over a pane's terminal, in one stack in a corner. Drag a header to move the stack: it
 * follows the pointer with a transform only, so the terminal never resizes or reflows, then snaps to the nearest
 * corner. The overlay lets clicks through except on the cards, and never takes focus from the terminal.
 */
export function PaneRails({ paneKey, entries, onOpen }: { paneKey: string; entries: PaneRailEntry[]; onOpen: (pluginId: string, page: string) => void }) {
  const corner = useSyncExternalStore(subscribe, () => get(CORNER_KEY)) as Corner | null;
  const minimised = useSyncExternalStore(subscribe, () => get(MINIMISED_KEY)) === "1";
  const [, rerender] = useState(0);
  const area = useRef<HTMLDivElement>(null);
  const stack = useRef<HTMLDivElement>(null);
  const at = CORNERS.includes(corner as Corner) ? (corner as Corner) : "top-right";

  const shown = entries.filter(({ pluginId, rail }) => dismissed.get(`${paneKey}:${pluginId}`) !== JSON.stringify(rail));
  if (shown.length === 0) return null;

  const grab = (e: React.PointerEvent) => {
    const el = stack.current;
    const box = area.current?.getBoundingClientRect();
    if (!el || !box || (e.target as HTMLElement).closest("[data-no-drag]")) return;
    const rest = el.getBoundingClientRect();
    trackDrag(e, {
      onStart: () => {
        el.style.willChange = "transform";
      },
      onMove: ({ dx, dy }) => {
        // Keep the whole stack inside the pane.
        const x = Math.min(Math.max(dx, box.left - rest.left), box.right - rest.right);
        const y = Math.min(Math.max(dy, box.top - rest.top), box.bottom - rest.bottom);
        el.style.transform = `translate3d(${x}px, ${y}px, 0)`;
      },
      onEnd: (_, cancelled) => {
        const first = el.getBoundingClientRect();
        el.style.transform = "";
        el.style.willChange = "";
        if (cancelled) return;
        // One render, once the gesture is over: the corner, then the card glides from where it was dropped.
        flushSync(() => write(CORNER_KEY, nearestCorner(first.left + first.width / 2 - box.left, first.top + first.height / 2 - box.top, box.width, box.height)));
        const now = el.getBoundingClientRect();
        if (reducedMotion()) return;
        el.animate([{ transform: `translate3d(${first.left - now.left}px, ${first.top - now.top}px, 0)` }, { transform: "translate3d(0, 0, 0)" }], { duration: 220, easing: "cubic-bezier(0.22, 1, 0.36, 1)" });
      },
    });
  };

  const [vertical, side] = at.split("-");
  return (
    <div
      ref={area}
      // Pointer events pass through the overlay to the terminal; only the cards take them. A press on a card keeps the terminal's focus.
      onMouseDown={(e) => e.preventDefault()}
      className={cn("pointer-events-none absolute inset-x-3.5 inset-y-3 z-10 flex flex-col", vertical === "top" ? "justify-start" : "justify-end", side === "left" ? "items-start" : "items-end")}
    >
      <motion.div ref={stack} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18 }} className={cn("pointer-events-none flex max-h-full max-w-full min-h-0 flex-col gap-2", side === "left" ? "items-start" : "items-end")}>
        {shown.map(({ pluginId, rail }) => (
          <Card
            key={pluginId}
            rail={rail}
            minimised={minimised}
            onMinimise={() => write(MINIMISED_KEY, minimised ? "0" : "1")}
            onClose={() => {
              dismissed.set(`${paneKey}:${pluginId}`, JSON.stringify(rail));
              rerender((n) => n + 1);
            }}
            onOpen={rail.open ? () => onOpen(pluginId, rail.open!) : undefined}
            onGrab={grab}
          />
        ))}
      </motion.div>
    </div>
  );
}
