import { useEffect, useRef, useState } from "react";
import { InboxIcon, PuzzleIcon } from "lucide-react";

import { SidebarNavRow } from "@/components/sidebar-nav-row";
import { StatusDot } from "@/components/status-dot";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { unreadCount, unreadPluginCount, type NotificationButton, type NotificationEvent } from "@/lib/notifications";
import { OPEN_NOTIFICATIONS } from "@/lib/ui-events";
import { cn } from "@/lib/utils";

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto", style: "short" });
/** Worked out when the list renders, so nothing ticks while it's closed. */
function ago(at: number) {
  const s = Math.round((Date.now() - at) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return relative.format(-Math.floor(s / 60), "minute");
  if (s < 86_400) return relative.format(-Math.floor(s / 3600), "hour");
  return relative.format(-Math.floor(s / 86_400), "day");
}

/** The sidebar's Inbox row: unread count on it, the last agent events in a popover beside it, newest first. */
export function NotificationBell({
  events,
  onOpen,
  onClear,
  onGoToPane,
  onButton,
  listen = true,
}: {
  events: NotificationEvent[];
  /** Called when the list opens: the app marks everything read. */
  onOpen: () => void;
  onClear: () => void;
  onGoToPane: (paneId: string) => void;
  /** A plugin notification's button was clicked. */
  onButton: (plugin: NonNullable<NotificationEvent["plugin"]>, button: NotificationButton) => void;
  /** Opens on the show-notifications command. Only one of the sidebar's two copies (docked, peeking) listens. */
  listen?: boolean;
}) {
  const [open, setOpen] = useState(false);
  /** What was unread when the list opened, so those rows stay marked while you look at them. */
  const [fresh, setFresh] = useState<Set<string>>(() => new Set());
  const unread = unreadCount(events);
  const accent = unreadPluginCount(events) > 0;
  // Each new unread arrival restarts a short pulse on the count (CSS, three beats, then still).
  const [pulse, setPulse] = useState(0);
  const last = useRef(unread);
  useEffect(() => {
    if (unread > last.current) setPulse((n) => n + 1);
    last.current = unread;
  }, [unread]);

  const show = (next: boolean) => {
    if (next) {
      setFresh(new Set(events.filter((e) => !e.read).map((e) => e.id)));
      onOpen();
    }
    setOpen(next);
  };
  useEffect(() => {
    if (!listen) return;
    const listener = () => show(true);
    window.addEventListener(OPEN_NOTIFICATIONS, listener);
    return () => window.removeEventListener(OPEN_NOTIFICATIONS, listener);
  });

  return (
    <Popover open={open} onOpenChange={show}>
      <PopoverTrigger
        render={
          <SidebarNavRow
            icon={<InboxIcon />}
            label="Inbox"
            aria-label={unread ? `Inbox, ${unread} unread` : "Inbox"}
            trailing={
              unread > 0 && (
                <span
                  key={pulse}
                  data-pulse={pulse > 0 || undefined}
                  className={cn(
                    "inbox-count flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full px-1 text-[10px] leading-none font-medium tabular-nums",
                    accent ? "bg-chart-1 text-primary-foreground" : "bg-white/[0.1] text-foreground",
                  )}
                  aria-hidden
                >
                  {unread > 99 ? "99+" : unread}
                </span>
              )
            }
          />
        }
      />
      <PopoverContent side="right" align="start" className="w-96 gap-0 p-0">
        <div className="flex items-center justify-between border-b border-hairline py-2 pr-2 pl-3.5">
          <h2 className="text-[13px] font-semibold">Notifications</h2>
          <Button variant="ghost" size="xs" disabled={events.length === 0} onClick={onClear} className="text-muted-foreground">
            Clear
          </Button>
        </div>
        {events.length === 0 ? (
          <Empty className="py-10">
            <EmptyHeader>
              <EmptyTitle className="text-[13px]">Nothing yet</EmptyTitle>
              <EmptyDescription>When an agent needs you or finishes, it shows up here.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <ScrollArea className="[&>[data-slot=scroll-area-viewport]]:max-h-96">
            {/* The cap goes on the viewport: a percentage height inside a max-height root resolves to auto and never scrolls. */}
            <ul className="flex flex-col p-1.5">
              {events.map((e) => (
                <li key={e.id} className={cn("flex items-start gap-2.5 rounded-lg px-2.5 py-2", fresh.has(e.id) && "bg-white/[0.04]")}>
                  <span className="mt-1 flex w-3 shrink-0 justify-center">
                    {e.kind === "plugin" ? <PuzzleIcon className="size-3.5 text-muted-foreground" aria-hidden /> : <StatusDot state={e.kind} />}
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="flex items-baseline gap-2 text-[13px]">
                      <span title={e.title} className="min-w-0 flex-1 truncate font-medium">{e.title}</span>
                      <time dateTime={new Date(e.at).toISOString()} className="shrink-0 text-[11px] text-muted-foreground">
                        {ago(e.at)}
                      </time>
                    </p>
                    <p title={[e.project, e.detail].filter(Boolean).join(": ")} className="line-clamp-2 text-[12px] text-muted-foreground">{[e.project, e.detail].filter(Boolean).join(": ")}</p>
                    {e.paneId !== null && (
                      <Button
                        variant="ghost"
                        size="xs"
                        className="mt-1 -ml-2 text-muted-foreground"
                        onClick={() => {
                          setOpen(false);
                          onGoToPane(e.paneId!);
                        }}
                      >
                        Go to pane
                      </Button>
                    )}
                    {e.plugin?.buttons.length ? (
                      <div className="mt-1 -ml-2 flex flex-wrap gap-x-1">
                        {e.plugin.buttons.map((b) => (
                          <Button
                            key={b.label}
                            variant="ghost"
                            size="xs"
                            className="text-muted-foreground"
                            onClick={() => {
                              setOpen(false);
                              onButton(e.plugin!, b);
                            }}
                          >
                            {b.label}
                          </Button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          </ScrollArea>
        )}
      </PopoverContent>
    </Popover>
  );
}
