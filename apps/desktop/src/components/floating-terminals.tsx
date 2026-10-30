import { useEffect, useRef, useState } from "react";
import { MinusIcon, PanelBottomIcon, SquareTerminalIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import type { PaneLabelInfo } from "@/components/pane-label";
import { trackDrag } from "@/lib/pointer-drag";
import { terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

/** Where the panel sits, as distances from the bottom-right corner of the pane, so a smaller window never leaves it out of view. */
type Rect = { right: number; bottom: number; w: number; h: number };
type PanelState = { rect: Rect; collapsed: boolean; active: string | null };

const MIN_W = 280;
const MIN_H = 120;
const EDGE = 12;
/** The panel of each agent pane, for as long as Wings runs: a restructured tree remounts the pane that shows it. */
const states = new Map<string, PanelState>();
const initial = (): PanelState => ({ rect: { right: EDGE, bottom: EDGE, w: 520, h: 300 }, collapsed: false, active: null });

/** One floated terminal. Its element moves in from the split it left and refits at the panel's size once that holds still. */
function Host({ paneKey, shown, visible, keepRenderer, initialInput }: { paneKey: string; shown: boolean; visible: boolean; keepRenderer: boolean; initialInput: string | null }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const term = terminals.get(paneKey);
    if (term && host.current) term.attach(host.current, initialInput);
  }, [paneKey, initialInput]);
  useEffect(() => {
    terminals.get(paneKey)?.setVisible(shown && visible, keepRenderer);
  }, [paneKey, shown, visible, keepRenderer]);
  return <div ref={host} className={cn("min-h-0 flex-1 pr-1 pb-0.5 pl-3", !shown && "hidden")} />;
}

/**
 * The terminals an agent opened, floated over its pane instead of split below it: drag the header to move the panel,
 * the corner to resize it, minimise it to a bar to glance at later, or close the terminal it shows. Moves and resizes touch only the panel's own
 * style, so the agent's terminal never reflows during a gesture, and the floated terminal refits once at the end.
 */
export function FloatingTerminals({ ownerKey, ownerTitle, commands, labels, visible, keepRenderer, initialInputs, onDock, onClose }: {
  ownerKey: string;
  ownerTitle: string;
  commands: string[];
  labels: Record<string, PaneLabelInfo>;
  visible: boolean;
  keepRenderer: boolean;
  initialInputs: Record<string, string | null>;
  onDock: () => void;
  onClose: (paneKey: string) => void;
}) {
  const [state, setState] = useState<PanelState>(() => states.get(ownerKey) ?? initial());
  const area = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const keep = (next: PanelState) => {
    states.set(ownerKey, next);
    setState(next);
  };
  const active = commands.includes(state.active ?? "") ? (state.active as string) : commands[commands.length - 1];
  const label = labels[active];
  const { rect, collapsed } = state;

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
        } else keep({ ...state, rect: next });
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
        } else keep({ ...state, rect: next });
      },
    }, 2);
  };

  const names = (key: string) => labels[key]?.title ?? "Terminal";
  const bodies = commands.map((key) => <Host key={key} paneKey={key} shown={!collapsed && key === active} visible={visible} keepRenderer={keepRenderer} initialInput={initialInputs[key] ?? null} />);

  return (
    <div
      ref={area}
      // Clicks pass through the overlay to the agent's terminal; only the panel takes them, and a press on it keeps its own focus.
      onMouseDown={(e) => e.stopPropagation()}
      className="pointer-events-none absolute inset-0 z-20"
    >
      <div
        ref={panel}
        role="group"
        aria-label={`Terminals opened by ${ownerTitle}`}
        style={{ right: rect.right, bottom: rect.bottom, ...(collapsed ? {} : { width: rect.w, height: rect.h }) }}
        className={cn(
          "pointer-events-auto absolute flex max-h-full max-w-full flex-col overflow-hidden border border-hairline-strong bg-surface text-[12px] shadow-lg",
          collapsed ? "h-7 rounded-full bg-popover/90 backdrop-blur-xl" : "rounded-xl",
        )}
      >
        <div
          onPointerDown={move}
          onClick={collapsed ? () => keep({ ...state, collapsed: false }) : undefined}
          className={cn("flex shrink-0 cursor-grab items-center gap-1.5 select-none active:cursor-grabbing", collapsed ? "h-7 pr-1 pl-3" : "h-8 pr-1 pl-3")}
        >
          <SquareTerminalIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          {commands.length > 1 && !collapsed ? (
            <div className="flex min-w-0 items-center gap-1" role="tablist">
              {commands.map((key) => (
                <button
                  key={key}
                  type="button"
                  role="tab"
                  data-no-drag
                  aria-selected={key === active}
                  onClick={() => keep({ ...state, active: key })}
                  className={cn("max-w-28 truncate rounded-md px-1.5 py-0.5 outline-none focus-visible:ring-1 focus-visible:ring-ring", key === active ? "bg-hover font-medium text-foreground" : "text-muted-foreground hover:text-foreground")}
                >
                  {names(key)}
                </button>
              ))}
            </div>
          ) : (
            <span className="min-w-0 truncate font-medium">{names(active)}</span>
          )}
          <span className="min-w-0 truncate text-muted-foreground">
            {collapsed && commands.length > 1 ? `+${commands.length - 1} · ` : ""}
            {label?.detail ? `${label.detail} · ` : ""}by {ownerTitle}
          </span>
          <span className="flex-1" />
          {collapsed ? (
            <>
              <IconButton label="Dock back into the split" data-no-drag className="size-5" onClick={onDock}>
                <PanelBottomIcon />
              </IconButton>
              {/* A click on the bar opens the panel: the others would pop open as this one closes. */}
              <IconButton
                label={`Close ${names(active)}`}
                data-no-drag
                className="size-5"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(active);
                }}
              >
                <XIcon />
              </IconButton>
            </>
          ) : (
            <>
              <IconButton label="Minimise to a bar" data-no-drag onClick={() => keep({ ...state, collapsed: true })}>
                <MinusIcon />
              </IconButton>
              <IconButton label="Dock back into the split" data-no-drag onClick={onDock}>
                <PanelBottomIcon />
              </IconButton>
              <IconButton label={`Close ${names(active)}`} data-no-drag onClick={() => onClose(active)}>
                <XIcon />
              </IconButton>
            </>
          )}
        </div>
        {/* The terminals stay mounted while minimised, hidden, so scrollback and the shell are untouched. */}
        <div className={cn("flex min-h-0 flex-1 flex-col", collapsed && "hidden")}>{bodies}</div>
        {!collapsed && <div onPointerDown={resize} aria-hidden className="absolute top-0 left-0 size-4 cursor-nwse-resize touch-none" />}
      </div>
    </div>
  );
}
