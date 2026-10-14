import { motion } from "motion/react";
import { useEffect, useRef } from "react";
import { AppWindowIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { PeekStrip, usePeek } from "@/components/sidebar-peek";
import { pagesOf, type PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";
import { PluginGlyph } from "@/components/plugin-glyph";
import { cn } from "@/lib/utils";


/** `id` is `pluginId:sidebarId`; `key` also changes when the plugin restarts, so its page loads again. */
export type SidebarRef = { plugin: PluginView; sidebarId: string; id: string; key: string };

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

/** The classes of the sidebar element: a column in its panel when docked, an overlay on the panes when peeking. */
const panelClass = (docked: boolean, peeking: boolean) =>
  cn(
    "flex flex-col",
    docked
      ? "h-full min-w-[250px] pb-2 pl-1"
      : cn("absolute inset-y-2 right-0 z-30 rounded-l-xl border border-r-0 bg-popover pt-1 pr-1 pb-2 pl-1 shadow-2xl transition-[visibility] duration-0", peeking ? "delay-0" : "invisible delay-300"),
  );

/**
 * What the sidebar shows and whether it floats. Docked, the open page. Closed, the page opened last. `onPeek` tells
 * App which page the peek shows, so it stays loaded.
 */
function useSidebarPeek(open: SidebarRef | null, mounted: SidebarRef[], onPeek: (id: string | null) => void) {
  const docked = open !== null;
  const peek = usePeek(!docked && mounted.length > 0);
  const last = useRef<string | null>(null);
  if (open) last.current = open.id;
  const current = open ?? mounted.find((e) => e.id === last.current) ?? mounted.at(-1) ?? null;
  const currentId = current?.id ?? null;
  useEffect(() => {
    onPeek(peek.shown ? currentId : null);
  }, [peek.shown, currentId]);
  return {
    docked,
    showing: docked || peek.shown,
    peek,
    current,
  };
}

/**
 * The right-hand sidebar for plugin pages, opened from their title bar buttons. It slides like the left
 * sidebar. Pages opened once stay alive while it's closed or showing another, so a timer keeps its state.
 *
 * While closed, the pointer at the window's right edge peeks it: the same element, with the same page frames, turns
 * into an overlay (absolute, over the panes) instead of moving into another one, because moving an iframe reloads
 * it. The panes keep their size. Needs the panel to let it overflow (see `data-peek`) and no positioned ancestor
 * between it and the row.
 */
export function RightSidebar(props: {
  open: SidebarRef | null;
  mounted: SidebarRef[];
  /** Ids of the mounted pages that were hidden long enough to unload. */
  unloaded: ReadonlySet<string>;
  host: PluginHost | null;
  onClose: () => void;
  /** Moves the open page to a window of its own. Offered for pages whose manifest allows it. */
  onPopOut: (entry: SidebarRef) => void;
  /** Width of the docked sidebar, which the peek uses too. */
  width: number;
  /** The page a peek shows, or null when it's hidden, so App keeps it loaded. */
  onPeek: (id: string | null) => void;
}) {
  const { docked, showing, peek, current } = useSidebarPeek(props.open, props.mounted, props.onPeek);
  const page = current ? sidebarOf(current) : undefined;
  const title = page?.title;
  return (
    <aside className="h-full" aria-hidden={!showing} aria-label={title}>
      {!docked && props.mounted.length > 0 && <PeekStrip side="right" events={peek.stripEvents} />}
      <motion.div
        ref={peek.panel}
        data-peek={docked ? undefined : ""}
        inert={!showing}
        className={panelClass(docked, peek.shown)}
        style={docked ? undefined : { width: props.width }}
        initial={false}
        animate={{ x: showing ? 0 : "100%" }}
        transition={{ type: "spring", duration: 0.25, bounce: 0 }}
        {...peek.panelEvents}
      >
        <div className="flex h-8 shrink-0 items-center gap-1 pr-2 pl-1">
          {/* Which page this is; the title bar's plugin buttons switch between the open ones. */}
          <h2 className="flex min-w-0 flex-1 items-center gap-2 pl-2 text-[12px] font-medium text-muted-foreground">
            <PluginGlyph name={page?.icon ?? ""} svg={page?.iconSvg} className="size-3.5 shrink-0" />
            <span className="truncate">{title}</span>
          </h2>
          {current && page?.opens.includes("window") && (
            <IconButton label="Open in a window" side="left" onClick={() => props.onPopOut(current)} className="size-6 shrink-0 [&_svg]:size-3.5">
              <AppWindowIcon />
            </IconButton>
          )}
          <IconButton label="Close sidebar" side="left" onClick={docked ? props.onClose : peek.hide} className="size-6 shrink-0 [&_svg]:size-3.5">
            <XIcon />
          </IconButton>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface">
          {props.mounted.map((entry) => (
            <Page key={entry.key} entry={entry} host={props.host} visible={entry.key === current?.key} loaded={!props.unloaded.has(entry.id)} />
          ))}
        </div>
      </motion.div>
    </aside>
  );
}
