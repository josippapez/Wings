import { useEffect, useRef } from "react";
import { AppWindowIcon, CalendarIcon, ChartColumnIcon, ClockIcon, GlobeIcon, ListIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { pagesOf, type PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";


/** `id` is `pluginId:sidebarId`; `key` also changes when the plugin restarts, so its page loads again. */
export type SidebarRef = { plugin: PluginView; sidebarId: string; id: string; key: string };

const icons: Record<string, typeof ClockIcon> = { clock: ClockIcon, globe: GlobeIcon, calendar: CalendarIcon, chart: ChartColumnIcon, list: ListIcon };
const sidebarOf = (entry: SidebarRef) => pagesOf(entry.plugin).find((s) => s.id === entry.sidebarId);

/**
 * One plugin page, mounted into its own box once and kept running while hidden, until `loaded` goes false (it was
 * hidden long enough to unload) and its frame goes. It loads again, and asks its plugin for its state, when shown.
 */
function Page({ entry, host, visible, loaded }: { entry: SidebarRef; host: PluginHost | null; visible: boolean; loaded: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  // Keyed on entry.key, which changes only when the plugin restarts, so a new plugins list doesn't reload it.
  useEffect(() => {
    if (!box.current || !host || !loaded) return;
    let remove: (() => void) | undefined;
    let gone = false;
    void host
      .mountPage(entry.plugin, entry.sidebarId, box.current, "sidebar")
      .then((r) => (gone ? r() : (remove = r)))
      .catch((e) => console.error(e));
    return () => {
      gone = true;
      remove?.();
    };
  }, [entry.key, host, loaded]);
  return <div ref={box} hidden={!visible} className="min-h-0 flex-1" />;
}

/**
 * The right-hand sidebar for plugin pages, opened from their title bar buttons. It slides like the left
 * sidebar. Pages opened once stay alive while it's closed or showing another, so a timer keeps its state.
 */
export function RightSidebar(props: {
  open: SidebarRef | null;
  mounted: SidebarRef[];
  /** Ids of the mounted pages that were hidden long enough to unload. */
  unloaded: ReadonlySet<string>;
  host: PluginHost | null;
  onSelect: (id: string) => void;
  /** Closes one plugin's page; the last one closes the sidebar. */
  onCloseTab: (id: string) => void;
  onClose: () => void;
  /** Moves the open page to a window of its own. Offered for pages whose manifest allows it. */
  onPopOut: (entry: SidebarRef) => void;
}) {
  const page = props.open ? sidebarOf(props.open) : undefined;
  const title = page?.title;
  return (
    <aside className={cn("h-full transition-opacity duration-200", !props.open && "opacity-0")} aria-hidden={!props.open} aria-label={title}>
      <div className="flex h-full min-w-[250px] flex-col pb-2 pl-1">
        <div className="flex h-8 shrink-0 items-center gap-1 pr-2 pl-1">
          {props.mounted.length > 1 ? (
            // Several plugins' pages are open: one tab each, scrolling sideways if they don't fit. Only the
            // tab strip comes from Tabs; the pages stay mounted below so hidden ones keep running.
            <Tabs value={props.open?.id ?? null} onValueChange={(id) => props.onSelect(String(id))} className="min-w-0 flex-1 gap-0 data-horizontal:flex-row">
              <TabsList aria-label="Plugin sidebars" className="h-7 w-full justify-start overflow-x-auto bg-transparent p-0 [scrollbar-width:none]">
                {props.mounted.map((entry) => {
                  const sidebar = sidebarOf(entry);
                  const Icon = icons[sidebar?.icon ?? ""] ?? GlobeIcon;
                  return (
                    <div key={entry.id} className="group/tab relative flex h-full shrink-0 items-center">
                      <TabsTrigger value={entry.id} className="h-6 flex-none pr-6 pl-2 text-[12px] data-active:bg-white/[0.08]">
                        <Icon className="size-3.5" aria-hidden />
                        {sidebar?.title}
                      </TabsTrigger>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Close ${sidebar?.title}`}
                        onClick={() => props.onCloseTab(entry.id)}
                        className="absolute right-0.5 size-5 text-muted-foreground opacity-0 group-hover/tab:opacity-100 focus-visible:opacity-100"
                      >
                        <XIcon aria-hidden />
                      </Button>
                    </div>
                  );
                })}
              </TabsList>
            </Tabs>
          ) : (
            <h2 className="min-w-0 flex-1 truncate pl-2 text-[12px] font-medium text-muted-foreground">{title}</h2>
          )}
          {props.open && page?.opens.includes("window") && (
            <IconButton label="Open in a window" side="left" onClick={() => props.open && props.onPopOut(props.open)} className="size-6 shrink-0 [&_svg]:size-3.5">
              <AppWindowIcon />
            </IconButton>
          )}
          <IconButton label="Close sidebar" side="left" onClick={props.onClose} className="size-6 shrink-0 [&_svg]:size-3.5">
            <XIcon />
          </IconButton>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface">
          {props.mounted.map((entry) => (
            <Page key={entry.key} entry={entry} host={props.host} visible={entry.key === props.open?.key} loaded={!props.unloaded.has(entry.id)} />
          ))}
        </div>
      </div>
    </aside>
  );
}
