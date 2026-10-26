import { Fragment, useEffect, useRef, useState, type ReactElement } from "react";
import { toast } from "sonner";
import { flushSync } from "react-dom";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  ChevronDownIcon,
  EllipsisIcon,
  FileTextIcon,
  ChevronRightIcon,
  RotateCcwClockIcon,
  PanelLeftIcon,
  PencilIcon,
  PlusIcon,
  XIcon,
  TerminalIcon,
} from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { InlineName } from "@/components/inline-name";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Button } from "@/components/ui/button";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { LayoutGlyph } from "@/components/layout-glyph";
import { PaneIcon, type PaneLabelInfo } from "@/components/pane-label";
import { StatusDot } from "@/components/status-dot";
import type { AgentState, GitStatus, Space } from "@/lib/api";
import { GitCounts } from "@/components/git-counts";
import type { LayoutNode } from "@/lib/layout";
import { POPOVER_UNLOAD_MS } from "@/lib/idle-unload";
import { wantsNewTab, type NewTabEntry } from "@/lib/plugin-pages";
import { trackDrag } from "@/lib/pointer-drag";
import { isMac, shortcutLabel } from "@/lib/terminal";
import { PluginGlyph } from "@/components/plugin-glyph";
import { cn } from "@/lib/utils";


/** Plugin buttons that show before the rest move into a menu. Pinned ones always show, and take these places first. */
const INLINE_BUTTONS = 3;

/** A plugin's title bar button: it opens one of its pages in the sidebar, a modal, a window or a tab, or its web panel. */
export type PluginButton = {
  key: string;
  pluginId: string;
  kind: "sidebar" | "modal" | "window" | "tab" | "panel";
  id: string;
  title: string;
  /** A built-in icon name, or empty for the plugin's own `iconSvg`. */
  icon: string;
  iconSvg?: string | null;
  /** Short text from the plugin, like a running timer. */
  label?: string;
  /** Shown when you point at the button. */
  rows?: { label: string; value: string }[];
  /** Colours the label amber or red when the plugin says it needs attention. */
  tone?: "warning" | "danger";
  /** Its sidebar or modal is open, or its tab is on screen. */
  active?: boolean;
  /** A sidebar's popover page, shown while you point at the button. `key` changes when the plugin restarts. */
  popover?: { width: number; height: number; key: string };
};

/** Mounts a sidebar button's popover page into `container`, hidden or not; resolves what removes it and tells it it is out of view. */
export type MountPopover = (button: PluginButton, container: HTMLElement, hidden: boolean) => Promise<{ remove: () => void; setHidden: (hidden: boolean) => void }>;

/** The plugin's popover page, loaded once and kept while the popover is closed, so it reopens at once. */
function PopoverPage({ button, mount, open }: { button: PluginButton; mount: MountPopover; open: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const page = useRef<{ setHidden: (hidden: boolean) => void }>(null);
  const hidden = useRef(!open);
  hidden.current = !open;
  useEffect(() => page.current?.setHidden(!open), [open]);
  // Keyed on the page's key only: a new plugins list or callback mustn't reload it.
  useEffect(() => {
    if (!box.current) return;
    let remove: (() => void) | undefined;
    let gone = false;
    void mount(button, box.current, hidden.current)
      .then((r) => {
        if (gone) return r.remove();
        remove = r.remove;
        page.current = r;
        r.setHidden(hidden.current);
      })
      .catch((e) => toast.error("Couldn't open that panel", { description: String(e) }));
    return () => {
      gone = true;
      page.current = null;
      remove?.();
    };
  }, [button.popover?.key]);
  return <div ref={box} className="size-full" />;
}

/**
 * A sidebar button with a popover page: pointing at it or focusing it shows the page under it, and clicking
 * still opens the sidebar. The page loads on first show and stays mounted, hidden, until it has been hidden for
 * POPOVER_UNLOAD_MS; it loads again on the next show.
 */
function WithPopover({ button, popover, mount, children }: { button: PluginButton; popover: NonNullable<PluginButton["popover"]>; mount: MountPopover; children: ReactElement }) {
  const [open, setOpen] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // Not over its own open sidebar, which shows the same and more. Closed rather than hidden, so it doesn't come
  // back when the sidebar closes with the pointer elsewhere.
  if (button.active && open) setOpen(false);
  useEffect(() => {
    if (open || !loaded) return;
    const timer = setTimeout(() => setLoaded(false), POPOVER_UNLOAD_MS);
    return () => clearTimeout(timer);
  }, [open, loaded]);
  return (
    <HoverCard
      open={open}
      onOpenChange={(next) => {
        if (next && button.active) return;
        setOpen(next);
        if (next) setLoaded(true);
      }}
    >
      <HoverCardTrigger delay={300} closeDelay={200} render={children} />
      {loaded && (
        <HoverCardContent
          keepMounted
          align="end"
          alignOffset={0}
          sideOffset={6}
          className="overflow-hidden p-0"
          style={{ width: `min(${popover.width}px, calc(100vw - 1rem))`, height: `min(${popover.height}px, calc(100vh - 4rem))` }}
        >
          <PopoverPage button={button} mount={mount} open={open} />
        </HoverCardContent>
      )}
    </HoverCard>
  );
}

/**
 * A terminal tab, where `state` rolls up every pane's agent so a blocked agent in a background split still shows on
 * the tab, or a plugin view open as a tab.
 */
export type TabView =
  | { id: string; label: PaneLabelInfo; /** What the tab shows: your name for it, or the automatic title made unique. */ title: string; named: boolean; layout: LayoutNode; focusedPane: string; state: AgentState | null; /** Programs its panes run instead of a shell prompt, like `node`. */ running: string[] }
  | { id: string; view: { title: string; icon: string; iconSvg?: string | null } }
  /** A project's page, which is Wings' own and not a plugin's. */
  | { id: string; project: { title: string } };

const spring = { type: "spring", stiffness: 520, damping: 38, mass: 0.8 } as const;

export function TitleBar(props: {
  /** Whether a project is open, which the + menu's Project page needs. */
  hasSpace: boolean;
  tabs: TabView[];
  activeTabId: string | null;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  /** Back and forward through the places visited; a button is disabled when there is nowhere to go. */
  canGoBack: boolean;
  canGoForward: boolean;
  onGoBack: () => void;
  onGoForward: () => void;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  /** An empty name goes back to the automatic one. */
  onRename: (id: string, name: string) => void;
  /** Moves a tab to a place among this project's tabs. */
  onMove: (id: string, index: number) => void;
  /** What the plain + button does. */
  onNew: () => void;
  /** The plain + opens the menu instead, anchored to it. */
  newAsk: boolean;
  /** Plugin pages the + menu offers after Terminal; the chevron shows only when there are some. */
  newTabEntries: NewTabEntry[];
  onNewTerminal: () => void;
  onNewPage: (entry: NewTabEntry, newTab?: boolean) => void;
  onNewProjectPage: () => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  const plus = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  /** A rename chosen in the context menu starts once the menu has closed, so closing doesn't take the focus back. */
  const renameAfterMenu = useRef<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const tabCount = useRef(props.tabs.length);
  useEffect(() => {
    const el = list.current;
    if (!el) return;
    // A new tab opens at the end, still scaling in, so its size isn't final yet: go to the end instead.
    if (props.tabs.length > tabCount.current) {
      el.scrollTo({ left: el.scrollWidth, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
    } else {
      el.querySelector('[role="tab"][aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
    tabCount.current = props.tabs.length;
  }, [props.activeTabId, props.tabs.length]);

  /** Dragging a tab sideways: it follows the pointer, and the others make room as it passes them. */
  function dragTab(e: React.PointerEvent, id: string) {
    const inner = e.currentTarget.closest<HTMLElement>("[data-tab-inner]");
    const outer = inner?.parentElement;
    if (!inner || !outer) return;
    const from = props.tabs.findIndex((t) => t.id === id);
    let at = from;
    let startLeft = 0;
    const place = (dx: number) => {
      inner.style.transform = `translateX(${dx - (outer.getBoundingClientRect().left - startLeft)}px)`;
    };
    trackDrag(e, {
      onStart: () => {
        flushSync(() => setDragId(id));
        startLeft = outer.getBoundingClientRect().left;
        inner.style.zIndex = "10";
        document.body.style.cursor = "grabbing";
      },
      onMove: ({ dx }) => {
        place(dx);
        const box = inner.getBoundingClientRect();
        const centre = box.left + box.width / 2;
        const others = [...(list.current?.querySelectorAll<HTMLElement>("[data-tab-id]") ?? [])].filter((el) => el !== outer);
        const next = others.filter((el) => {
          const r = el.getBoundingClientRect();
          return r.left + r.width / 2 < centre;
        }).length;
        if (next === at) return;
        at = next;
        // Committed now, so the slot is where it will stay before the offset is worked out again.
        flushSync(() => props.onMove(id, next));
        place(dx);
      },
      onEnd: (_, cancelled) => {
        if (cancelled && at !== from) flushSync(() => props.onMove(id, from));
        document.body.style.cursor = "";
        inner.style.transition = "transform 160ms cubic-bezier(0.22, 1, 0.36, 1)";
        inner.style.transform = "translateX(0)";
        setTimeout(() => {
          inner.style.transition = inner.style.transform = inner.style.zIndex = "";
          setDragId(null);
        }, 170);
      },
    });
  }

  return (
    <header
      data-tauri-drag-region="deep"
      data-title-bar=""
      // --main-r is how far the main pane's right edge is from the window's (App keeps it current), so the strip
      // stops there when the right sidebar is open.
      className="relative flex h-10 shrink-0 items-center pr-[calc(var(--main-r,0px)+0.625rem)]"
    >
      {/* Traffic lights and the toggle stay over the sidebar. Open, this part is as wide as the sidebar (--main-l is
          the main pane's left edge, which App keeps current while the sidebar is dragged) plus the project bar's
          padding, so the tabs start where the project name does. */}
      <div className={cn("flex h-full shrink-0 items-center gap-1 pl-[84px]", props.sidebarOpen ? "min-w-[calc(var(--main-l,0px)+0.75rem)]" : "pr-1")}>
        <IconButton label={props.sidebarOpen ? "Hide sidebar" : "Show sidebar"} shortcut={shortcutLabel.toggleSidebar} onClick={props.onToggleSidebar}>
          <PanelLeftIcon />
        </IconButton>
        <IconButton label="Back" shortcut={shortcutLabel.goBack} disabled={!props.canGoBack} onClick={props.onGoBack}>
          <ArrowLeftIcon />
        </IconButton>
        <IconButton label="Forward" shortcut={shortcutLabel.goForward} disabled={!props.canGoForward} onClick={props.onGoForward}>
          <ArrowRightIcon />
        </IconButton>
      </div>

      <LayoutGroup id="tabs">
        {/* Base UI Tabs for the tab strip (arrow keys move between tabs); the sliding pill is ours. */}
        <Tabs value={props.activeTabId} onValueChange={(id) => props.onSelect(String(id))} className="min-w-0 flex-1 items-center gap-0 data-horizontal:flex-row">
          {/* Scrolls sideways when tabs don't fit, without scrollbars: WebKit flashed them each time a tab
              animated in. The active tab scrolls into view instead. */}
          <TabsList
            ref={list}
            variant="line"
            aria-label="Tabs"
            // A mouse wheel only scrolls up and down, and the strip only scrolls sideways.
            onWheel={(e) => {
              if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
            }}
            className="h-auto min-w-0 justify-start gap-0.5 overflow-x-auto overflow-y-hidden p-0 [scrollbar-width:none]"
          >
            <AnimatePresence initial={false} mode="popLayout">
              {props.tabs.map((tab, i) => {
                const active = tab.id === props.activeTabId;
                const title = "view" in tab ? tab.view.title : "project" in tab ? tab.project.title : tab.title;
                return (
                  <motion.div
                    key={tab.id}
                    data-tab-id={tab.id}
                    // A pane dropped on a terminal tab moves into it.
                    data-pane-drop={"view" in tab || "project" in tab ? undefined : ""}
                    layout={dragId === tab.id ? false : "position"}
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={spring}
                    className="group/tab relative flex shrink-0 rounded-lg data-drop-over:bg-white/[0.1] data-drop-over:ring-1 data-drop-over:ring-ring"
                  >
                    <ContextMenu
                      onOpenChangeComplete={(open) => {
                        if (open || renameAfterMenu.current !== tab.id) return;
                        renameAfterMenu.current = null;
                        setEditing(tab.id);
                      }}
                    >
                      <ContextMenuTrigger render={<div data-tab-inner className="relative flex" />}>
                        {active && (
                          <motion.span
                            // Plain while its tab is dragged, so it can't slide against the tab it belongs to.
                            layoutId={dragId === tab.id ? undefined : "active-tab"}
                            transition={spring}
                            className="absolute inset-0 rounded-lg border border-hairline-strong bg-white/[0.07] shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_1px_3px_rgb(0_0_0/0.25)]"
                          />
                        )}
                        {editing === tab.id && "label" in tab ? (
                          <span className="relative flex h-7 items-center gap-2 pr-2 pl-2.5">
                            <PaneIcon agent={tab.label.agent} />
                            <InlineName
                              initial={tab.title}
                              label="Tab name"
                              className="h-5 w-36 text-[12px] font-medium"
                              onDone={(name) => {
                                setEditing(null);
                                if (name !== null) props.onRename(tab.id, name);
                              }}
                            />
                          </span>
                        ) : (
                          <TabsTrigger
                            value={tab.id}
                            render={<motion.button type="button" whileTap={{ scale: 0.97 }} transition={spring} />}
                            onAuxClick={(e) => e.button === 1 && props.onClose(tab.id)}
                            onPointerDown={(e) => dragTab(e, tab.id)}
                            onDoubleClick={() => "label" in tab && setEditing(tab.id)}
                            title={`${title}  ${isMac ? "⌘" : "Ctrl+Shift+"}${i + 1}${"running" in tab && tab.running.length ? `\nRunning ${tab.running.join(", ")}` : ""}`}
                            className={cn(
                              "relative h-7 max-w-48 flex-none justify-start gap-1.5 rounded-lg border-0 pr-7 pl-2.5 text-[12px] font-normal after:hidden",
                              active ? "text-foreground" : "text-muted-foreground hover:bg-hover hover:text-foreground",
                            )}
                          >
                            {"view" in tab ? (
                              <PluginGlyph name={tab.view.icon} svg={tab.view.iconSvg} className="size-3.5 shrink-0 text-muted-foreground" />
                            ) : "project" in tab ? (
                              <FileTextIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                            ) : tab.state && !tab.label.agent ? (
                              <StatusDot state={tab.state} />
                            ) : (
                              <PaneIcon agent={tab.label.agent} />
                            )}
                            <span className="truncate font-medium">{title}</span>
                            {"running" in tab && tab.running.length > 0 && (
                              <span role="img" aria-label={`Running ${tab.running.join(", ")}`} className="size-1.5 shrink-0 rounded-full bg-done motion-safe:animate-pulse" />
                            )}
                            {"label" in tab && tab.label.detail && !tab.named && (
                              <span className="hidden truncate text-muted-foreground 2xl:inline">{tab.label.detail}</span>
                            )}
                            {"layout" in tab && <LayoutGlyph layout={tab.layout} focused={tab.focusedPane} />}
                          </TabsTrigger>
                        )}
                        <IconButton
                          label="Close tab"
                          onClick={() => props.onClose(tab.id)}
                          className="absolute top-1 right-1 size-5 opacity-0 transition-opacity group-hover/tab:opacity-100 focus-visible:opacity-100 [&_svg]:size-3.5"
                        >
                          <XIcon />
                        </IconButton>
                      </ContextMenuTrigger>
                      <ContextMenuContent side="bottom">
                        {"label" in tab && (
                          <ContextMenuItem onClick={() => (renameAfterMenu.current = tab.id)}>
                            <PencilIcon />
                            Rename
                          </ContextMenuItem>
                        )}
                        {"label" in tab && tab.named && (
                          <ContextMenuItem onClick={() => props.onRename(tab.id, "")}>
                            <RotateCcwClockIcon />
                            Use automatic name
                          </ContextMenuItem>
                        )}
                        <ContextMenuItem disabled={i === 0} onClick={() => props.onMove(tab.id, i - 1)}>
                          <ArrowLeftIcon />
                          Move left
                        </ContextMenuItem>
                        <ContextMenuItem disabled={i === props.tabs.length - 1} onClick={() => props.onMove(tab.id, i + 1)}>
                          <ArrowRightIcon />
                          Move right
                        </ContextMenuItem>
                        <ContextMenuSeparator />
                        <ContextMenuItem variant="destructive" onClick={() => props.onClose(tab.id)}>
                          <XIcon />
                          Close tab
                        </ContextMenuItem>
                      </ContextMenuContent>
                    </ContextMenu>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </TabsList>
          <IconButton ref={plus} label="New tab" shortcut={shortcutLabel.newTab} onClick={props.newAsk ? () => setMenuOpen(true) : props.onNew} data-pane-drop-new="" className="ml-1 data-drop-over:bg-white/20 data-drop-over:text-foreground">
            <PlusIcon />
          </IconButton>
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-sm" aria-label="New tab menu" title="New tab menu" className="-ml-1 w-4 text-muted-foreground hover:text-foreground" />}
            >
              <ChevronDownIcon className="size-3" aria-hidden />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" anchor={props.newAsk ? plus : undefined}>
              <DropdownMenuItem onClick={props.onNewTerminal} disabled={!props.hasSpace}>
                <TerminalIcon aria-hidden />
                <span className="flex-1 whitespace-nowrap">Terminal</span>
              </DropdownMenuItem>
              <DropdownMenuItem onClick={props.onNewProjectPage} disabled={!props.hasSpace}>
                <FileTextIcon aria-hidden />
                <span className="flex-1 whitespace-nowrap">Project page</span>
              </DropdownMenuItem>
              {props.newTabEntries.map((entry) => (
                <DropdownMenuItem
                  key={entry.key}
                  disabled={!props.hasSpace && !entry.noProject}
                  onClick={(e) => props.onNewPage(entry, wantsNewTab(e))}
                  onAuxClick={(e) => {
                    if (e.button !== 1) return;
                    e.preventDefault();
                    props.onNewPage(entry, true);
                  }}
                >
                  <PluginGlyph name={entry.icon} svg={entry.iconSvg} />
                  <span className="flex-1 whitespace-nowrap">{entry.title}</span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </Tabs>
      </LayoutGroup>

    </header>
  );
}

/** The tone dot before a plugin label: green with no tone, amber or red when the plugin needs attention. Pulses only for live labels like a timer. */
function ToneDot({ tone }: { tone: PluginButton["tone"] }) {
  return <span className={cn("size-1.5 shrink-0 rounded-full", tone === "warning" ? "bg-working" : tone === "danger" ? "bg-blocked" : "bg-done motion-safe:animate-pulse")} aria-hidden />;
}

/** The slim bar under the tabs, inside the main area: project › branch on the left, plugin buttons on the right. */
export function ProjectBar(props: {
  space: Space | null;
  git?: GitStatus | null;
  /** Sidebar and panel buttons from plugins that are on. Pinned ones and the first few show, the rest go in a menu. */
  pluginButtons: PluginButton[];
  /** Keys of the buttons you pinned to the bar, in the order you pinned them. */
  pinned: string[];
  onTogglePin: (key: string) => void;
  onPluginButton: (button: PluginButton, rect: DOMRect, newTab?: boolean) => void;
  onMountPopover: MountPopover;
}) {
  const pinned = props.pinned.flatMap((key) => props.pluginButtons.filter((b) => b.key === key));
  const rest = props.pluginButtons.filter((b) => !props.pinned.includes(b.key));
  const free = Math.max(0, INLINE_BUTTONS - pinned.length);
  const inline = [...pinned, ...rest.slice(0, free)];
  const overflow = rest.slice(free);
  const more = useRef<HTMLButtonElement>(null);
  const { space, git } = props;
  return (
    <div role="toolbar" aria-label="Project" data-testid="project-bar" className="flex h-9 shrink-0 items-center gap-1 border-b border-hairline pr-2 pl-3">
      {space && (
        <div className="mr-2 flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden" title={space.path}>
          <span className="min-w-0 shrink truncate text-[13px] font-semibold">{space.name}</span>
          {space.branch && (
            <>
              <ChevronRightIcon className="size-3 shrink-0 text-muted-foreground/60" aria-hidden />
              <span className="min-w-0 shrink-[2] truncate text-[12px] text-muted-foreground">{space.branch}</span>
            </>
          )}
          {git && (
            <GitCounts
              tinted
              counts={[
                { icon: "changed" as const, value: git.changed },
                { icon: "push" as const, value: git.ahead },
                { icon: "pull" as const, value: git.behind },
              ].filter((c) => c.value > 0)}
            />
          )}
        </div>
      )}
      {!space && <div className="flex-1" />}
      {inline.map((button) => {
        const icon = (
          <IconButton
            key={button.key}
            // The popover shows more than the tooltip would.
            tooltipDisabled={button.popover !== undefined}
            label={button.label ? `${button.title}, ${button.label}` : button.title}
            tooltip={
              button.rows?.length ? (
                <div className="flex flex-col gap-1.5 py-0.5">
                  <span className="font-medium">{button.title}</span>
                  <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5">
                    {button.rows.map((row) => (
                      <Fragment key={row.label}>
                        <dt className="opacity-60">{row.label}</dt>
                        <dd className="truncate">{row.value}</dd>
                      </Fragment>
                    ))}
                  </dl>
                </div>
              ) : undefined
            }
            aria-pressed={button.kind === "sidebar" || button.kind === "modal" || button.kind === "tab" ? button.active : undefined}
            onClick={(e) => props.onPluginButton(button, e.currentTarget.getBoundingClientRect(), wantsNewTab(e))}
            // A middle click opens a tab view in a new tab; preventDefault stops autoscroll.
            onAuxClick={(e) => {
              if (e.button !== 1 || button.kind !== "tab") return;
              e.preventDefault();
              props.onPluginButton(button, e.currentTarget.getBoundingClientRect(), true);
            }}
            className={cn(
              // A label means something is live, like a running timer: small text after the icon, behind a tone dot.
              button.label && "w-auto gap-1.5 px-2",
              button.active && "bg-white/[0.08] text-foreground",
            )}
          >
            <PluginGlyph name={button.icon} svg={button.iconSvg} />
            {button.label && <ToneDot tone={button.tone} />}
            {button.label && <span className="max-w-[200px] truncate text-[12px] tabular-nums">{button.label}</span>}
          </IconButton>
        );
        return button.popover ? (
          // Keyed on the page too, so a restarted plugin loads its popover again on the next show.
          <WithPopover key={button.popover.key} button={button} popover={button.popover} mount={props.onMountPopover}>
            {icon}
          </WithPopover>
        ) : (
          icon
        );
      })}
      {(overflow.length > 0 || pinned.length > 0) && (
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button ref={more} variant="ghost" size="icon-sm" aria-label={overflow.length ? `${overflow.length} more from plugins` : "Plugin buttons"} title="Plugin buttons" className="text-muted-foreground hover:text-foreground" />
            }
          >
            <EllipsisIcon aria-hidden />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {overflow.map((button) => {
              return (
                <DropdownMenuItem
                  key={button.key}
                  onClick={(e) => props.onPluginButton(button, more.current?.getBoundingClientRect() ?? new DOMRect(), wantsNewTab(e))}
                  onAuxClick={(e) => {
                    if (e.button !== 1 || button.kind !== "tab") return;
                    e.preventDefault();
                    props.onPluginButton(button, more.current?.getBoundingClientRect() ?? new DOMRect(), true);
                  }}
                >
                  <PluginGlyph name={button.icon} svg={button.iconSvg} />
                  <span className="flex-1 whitespace-nowrap">{button.title}</span>
                  {button.label && (
                    <span className="ml-3 flex max-w-[200px] items-center gap-1.5 text-[12px] whitespace-nowrap text-muted-foreground tabular-nums">
                      <ToneDot tone={button.tone} />
                      <span className="truncate">{button.label}</span>
                    </span>
                  )}
                </DropdownMenuItem>
              );
            })}
            {overflow.length > 0 && <DropdownMenuSeparator />}
            <DropdownMenuGroup>
              <DropdownMenuLabel>Keep in bar</DropdownMenuLabel>
              {props.pluginButtons.map((button) => (
                <DropdownMenuCheckboxItem key={button.key} checked={props.pinned.includes(button.key)} onCheckedChange={() => props.onTogglePin(button.key)}>
                  {button.title}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}
