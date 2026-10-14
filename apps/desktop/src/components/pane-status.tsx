import { toneClass } from "@/components/pane-badge";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card";
import type { PaneStatus } from "@/lib/pane-status";
import { cn } from "@/lib/utils";

/** Fixed, so an update never changes the terminal's rows. Only the strip appearing or going away does. */
export const STATUS_STRIP_HEIGHT = 22;

export type PaneStatusEntry = { pluginId: string; status: PaneStatus };

/** One plugin's items: a button when the status names a page to `open`. Hovering shows its `rows`. */
function Group({ status, onOpen }: { status: PaneStatus; onOpen: (page: string) => void }) {
  const items = status.items.map((item, i) => (
    <span key={i} className="flex h-[22px] shrink-0 items-center gap-1.5 whitespace-nowrap">
      {item.label && <span className="text-muted-foreground">{item.label}</span>}
      {item.percent !== undefined && (
        <span className="h-[3px] w-10 shrink-0 overflow-hidden rounded-full bg-white/10" aria-hidden>
          <span
            // Width only changes when a value does; no transition-all inside panes.
            className={cn("block h-full rounded-full", toneClass[item.tone ?? "neutral"].dot)}
            style={{ width: `${item.percent}%` }}
          />
        </span>
      )}
      <span className={cn("tabular-nums", item.tone && item.tone !== "neutral" ? toneClass[item.tone].text : "text-foreground/90")}>{item.value}</span>
    </span>
  ));
  // Items that don't fit wrap to a row below the strip's fixed height (each is a full 22px row), which clips them whole instead of cutting text.
  const className = "flex h-full min-w-0 flex-wrap content-start items-center gap-x-3 overflow-hidden outline-none";
  const body = status.open ? (
    <button type="button" onClick={() => onOpen(status.open!)} className={cn(className, "cursor-pointer rounded-sm hover:brightness-125 focus-visible:ring-1 focus-visible:ring-ring")}>
      {items}
    </button>
  ) : (
    <div className={className}>{items}</div>
  );
  if (!status.rows?.length) return body;
  return (
    <HoverCard>
      <HoverCardTrigger delay={300} closeDelay={150} render={body} />
      <HoverCardContent side="top" align="start" sideOffset={8} className="w-72 border-0 bg-popover/95 p-3 backdrop-blur-xl">
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-[12px]">
          {status.rows.map((row, i) => (
            <div key={i} className="contents">
              <dt className="text-muted-foreground">{row.label}</dt>
              <dd className={cn("flex min-w-0 items-center gap-1.5", row.tone && toneClass[row.tone].text)}>
                {row.tone && <span className={cn("size-1.5 shrink-0 rounded-full", toneClass[row.tone].dot)} aria-hidden />}
                <span className="truncate" title={row.value}>
                  {row.value}
                </span>
              </dd>
            </div>
          ))}
        </dl>
      </HoverCardContent>
    </HoverCard>
  );
}

/** The strip under a pane's terminal, where plugins show figures that should always be in view. */
export function PaneStatusStrip({ entries, onOpen }: { entries: PaneStatusEntry[]; onOpen: (pluginId: string, page: string) => void }) {
  return (
    <div
      data-pane-status
      style={{ height: STATUS_STRIP_HEIGHT }}
      className="flex shrink-0 flex-wrap content-start items-center gap-x-3 overflow-hidden border-t border-hairline pr-3 pl-3.5 text-[11px] leading-none"
    >
      {entries.map(({ pluginId, status }, i) => (
        <div key={pluginId} className="flex h-full max-w-full min-w-0 items-center gap-3">
          {i > 0 && <span className="size-[3px] shrink-0 rounded-full bg-white/20" aria-hidden />}
          <Group status={status} onOpen={(page) => onOpen(pluginId, page)} />
        </div>
      ))}
    </div>
  );
}
