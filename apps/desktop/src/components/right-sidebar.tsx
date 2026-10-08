import { useEffect, useRef } from "react";
import { motion } from "motion/react";
import { CalendarIcon, ChartColumnIcon, ClockIcon, GlobeIcon, ListIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";

const WIDTH = 360;
const spring = { type: "spring", stiffness: 520, damping: 40, mass: 0.8 } as const;

/** `id` is `pluginId:sidebarId`; `key` also changes when the plugin restarts, so its page loads again. */
export type SidebarRef = { plugin: PluginView; sidebarId: string; id: string; key: string };

const icons: Record<string, typeof ClockIcon> = { clock: ClockIcon, globe: GlobeIcon, calendar: CalendarIcon, chart: ChartColumnIcon, list: ListIcon };
const sidebarOf = (entry: SidebarRef) => entry.plugin.contributes.sidebars.find((s) => s.id === entry.sidebarId);

/** One plugin page, mounted into its own box once and kept running while hidden. */
function Page({ entry, host, visible }: { entry: SidebarRef; host: PluginHost | null; visible: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  // Keyed on entry.key, which changes only when the plugin restarts, so a new plugins list doesn't reload it.
  useEffect(() => {
    if (box.current && host) void host.mountSidebar(entry.plugin, entry.sidebarId, box.current).catch((e) => console.error(e));
  }, [entry.key, host]);
  return <div ref={box} hidden={!visible} className="min-h-0 flex-1" />;
}

/**
 * The right-hand sidebar for plugin pages, opened from their title bar buttons. It slides like the left
 * sidebar. Pages opened once stay alive while it's closed or showing another, so a timer keeps its state.
 */
export function RightSidebar(props: {
  open: SidebarRef | null;
  mounted: SidebarRef[];
  host: PluginHost | null;
  onSelect: (id: string) => void;
  /** Closes one plugin's page; the last one closes the sidebar. */
  onCloseTab: (id: string) => void;
  onClose: () => void;
}) {
  const title = props.open ? sidebarOf(props.open)?.title : undefined;
  return (
    <motion.aside
      initial={false}
      animate={{ width: props.open ? WIDTH : 0, opacity: props.open ? 1 : 0 }}
      transition={spring}
      className="shrink-0 overflow-hidden"
      aria-hidden={!props.open}
      aria-label={title}
    >
      <div className={cn("flex h-full flex-col pb-2 pl-1")} style={{ width: WIDTH }}>
        <div className="flex h-8 shrink-0 items-center gap-1 pr-2 pl-1">
          {props.mounted.length > 1 ? (
            // Several plugins' pages are open: one tab each, scrolling sideways if they don't fit. Only the
            // tab strip comes from Tabs; the pages stay mounted below so hidden ones keep running.
            <Tabs value={props.open?.id ?? null} onValueChange={(id) => props.onSelect(String(id))} className="min-w-0 flex-1 gap-0">
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
                      <button
                        type="button"
                        aria-label={`Close ${sidebar?.title}`}
                        onClick={() => props.onCloseTab(entry.id)}
                        className="absolute right-1 flex size-4 items-center justify-center rounded text-muted-foreground opacity-0 transition-opacity outline-none group-hover/tab:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
                      >
                        <XIcon className="size-3" aria-hidden />
                      </button>
                    </div>
                  );
                })}
              </TabsList>
            </Tabs>
          ) : (
            <h2 className="min-w-0 flex-1 truncate pl-2 text-[12px] font-medium text-muted-foreground">{title}</h2>
          )}
          <IconButton label="Close sidebar" side="left" onClick={props.onClose} className="size-6 shrink-0 [&_svg]:size-3.5">
            <XIcon />
          </IconButton>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface">
          {props.mounted.map((entry) => (
            <Page key={entry.key} entry={entry} host={props.host} visible={entry.key === props.open?.key} />
          ))}
        </div>
      </div>
    </motion.aside>
  );
}
