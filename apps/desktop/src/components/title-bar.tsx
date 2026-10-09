import { Fragment, useEffect, useRef } from "react";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import {
  CalendarIcon,
  ChartColumnIcon,
  ClockIcon,
  EllipsisIcon,
  GlobeIcon,
  RotateCcwClockIcon,
  ListIcon,
  PanelLeftIcon,
  PlusIcon,
  PuzzleIcon,
  XIcon,
  SettingsIcon,
} from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { LayoutGlyph } from "@/components/layout-glyph";
import { PaneIcon, type PaneLabelInfo } from "@/components/pane-label";
import { StatusDot } from "@/components/status-dot";
import type { AgentState, Space } from "@/lib/api";
import type { LayoutNode } from "@/lib/layout";
import { isMac, shortcutLabel } from "@/lib/terminal";
import { cn } from "@/lib/utils";

const panelIcons: Record<string, typeof ClockIcon> = { clock: ClockIcon, globe: GlobeIcon, calendar: CalendarIcon, chart: ChartColumnIcon, list: ListIcon };

/** Plugin buttons that fit before the rest move into a menu. */
const INLINE_BUTTONS = 3;

/** A plugin's title bar button: it opens its sidebar or its web panel. */
export type PluginButton = {
  key: string;
  pluginId: string;
  kind: "sidebar" | "panel";
  id: string;
  title: string;
  icon: string;
  /** Short text from the plugin, like a running timer. */
  label?: string;
  /** Shown when you point at the button. */
  rows?: { label: string; value: string }[];
  /** Colours the label amber or red when the plugin says it needs attention. */
  tone?: "warning" | "danger";
  /** Its sidebar is open. */
  active?: boolean;
};

/** `state` rolls up every pane's agent, so a blocked agent in a background split still shows on the tab. */
export type TabView = { id: string; label: PaneLabelInfo; layout: LayoutNode; focusedPane: string; state: AgentState | null };

const spring = { type: "spring", stiffness: 520, damping: 38, mass: 0.8 } as const;

export function TitleBar(props: {
  space: Space | null;
  tabs: TabView[];
  activeTabId: string | null;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNew: () => void;
  onHistory: () => void;
  onPlugins: () => void;
  onSettings: () => void;
  /** Sidebar and panel buttons from plugins that are on. The first few show, the rest go in a menu. */
  pluginButtons: PluginButton[];
  onPluginButton: (button: PluginButton, rect: DOMRect) => void;
}) {
  const inline = props.pluginButtons.slice(0, INLINE_BUTTONS);
  const overflow = props.pluginButtons.slice(INLINE_BUTTONS);
  const more = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
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
  return (
    <header
      data-tauri-drag-region="deep"
      className="relative flex h-12 shrink-0 items-center gap-1.5 pr-2.5 pl-[84px]"
    >
      <IconButton label={props.sidebarOpen ? "Hide sidebar" : "Show sidebar"} shortcut={shortcutLabel.toggleSidebar} onClick={props.onToggleSidebar}>
        <PanelLeftIcon />
      </IconButton>

      {props.space && (
        <div className="mr-2 ml-1 flex min-w-0 max-w-44 flex-col leading-tight">
          <span className="truncate text-[13px] font-semibold">{props.space.name}</span>
          {props.space.branch && <span className="truncate text-[11px] text-muted-foreground">{props.space.branch}</span>}
        </div>
      )}

      <LayoutGroup id="tabs">
        {/* Base UI Tabs for the tab strip (arrow keys move between tabs); the sliding pill is ours. */}
        <Tabs value={props.activeTabId} onValueChange={(id) => props.onSelect(String(id))} className="min-w-0 flex-1 items-center gap-0 data-horizontal:flex-row">
          {/* Scrolls sideways when tabs don't fit, without scrollbars: WebKit flashed them each time a tab
              animated in. The active tab scrolls into view instead. */}
          <TabsList
            ref={list}
            variant="line"
            aria-label="Terminals"
            className="h-auto min-w-0 justify-start gap-0.5 overflow-x-auto overflow-y-hidden p-0 [scrollbar-width:none]"
          >
            <AnimatePresence initial={false} mode="popLayout">
              {props.tabs.map((tab, i) => {
                const active = tab.id === props.activeTabId;
                return (
                  <motion.div
                    key={tab.id}
                    layout="position"
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={spring}
                    className="group/tab relative flex shrink-0"
                  >
                    {active && (
                      <motion.span
                        layoutId="active-tab"
                        transition={spring}
                        className="absolute inset-0 rounded-[10px] border border-hairline-strong bg-white/[0.07] shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_1px_3px_rgb(0_0_0/0.25)]"
                      />
                    )}
                    <TabsTrigger
                      value={tab.id}
                      render={<motion.button type="button" whileTap={{ scale: 0.97 }} transition={spring} />}
                      onAuxClick={(e) => e.button === 1 && props.onClose(tab.id)}
                      title={`${tab.label.title}  ${isMac ? "⌘" : "Ctrl+Shift+"}${i + 1}`}
                      className={cn(
                        "relative h-8 max-w-60 flex-none justify-start gap-2 rounded-[10px] border-0 pr-8 pl-3 text-[13px] font-normal after:hidden",
                        active ? "text-foreground" : "text-muted-foreground hover:bg-hover hover:text-foreground",
                      )}
                    >
                      {tab.state && !tab.label.agent ? <StatusDot state={tab.state} /> : <PaneIcon agent={tab.label.agent} />}
                      <span className="truncate font-medium">{tab.label.title}</span>
                      {tab.label.detail && (
                        <span className="hidden truncate text-muted-foreground xl:inline">{tab.label.detail}</span>
                      )}
                      <LayoutGlyph layout={tab.layout} focused={tab.focusedPane} />
                    </TabsTrigger>
                    <IconButton
                      label="Close tab"
                      onClick={() => props.onClose(tab.id)}
                      className="absolute top-1.5 right-1.5 size-5 opacity-0 transition-opacity group-hover/tab:opacity-100 focus-visible:opacity-100 [&_svg]:size-3.5"
                    >
                      <XIcon />
                    </IconButton>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </TabsList>
          <IconButton label="New tab" shortcut={shortcutLabel.newTab} onClick={props.onNew} className="ml-1">
            <PlusIcon />
          </IconButton>
        </Tabs>
      </LayoutGroup>

      {inline.map((button) => {
        const Icon = panelIcons[button.icon] ?? GlobeIcon;
        return (
          <IconButton
            key={button.key}
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
            aria-pressed={button.kind === "sidebar" ? button.active : undefined}
            onClick={(e) => props.onPluginButton(button, e.currentTarget.getBoundingClientRect())}
            className={cn(
              // A label means something is live, like a running timer, so it shows as a green pill like a pane badge.
              button.label && "h-6 w-auto gap-1.5 rounded-full px-2.5 ring-1 ring-inset",
              button.label && !button.tone && "bg-done/12 text-done ring-done/25 hover:bg-done/20 hover:text-done",
              button.label && button.tone === "warning" && "bg-working/12 text-working ring-working/25 hover:bg-working/20 hover:text-working",
              button.label && button.tone === "danger" && "bg-blocked/12 text-blocked ring-blocked/30 hover:bg-blocked/20 hover:text-blocked",
              button.active && !button.label && "bg-white/[0.08] text-foreground",
              button.active && button.label && !button.tone && "bg-done/20",
              button.active && button.tone === "warning" && "bg-working/20",
              button.active && button.tone === "danger" && "bg-blocked/20",
            )}
          >
            {button.label && (
              <span
                className={cn("size-1.5 shrink-0 rounded-full motion-safe:animate-pulse", button.tone === "warning" ? "bg-working" : button.tone === "danger" ? "bg-blocked" : "bg-done")}
                aria-hidden
              />
            )}
            <Icon />
            {button.label && <span className="text-[12px] font-medium tabular-nums">{button.label}</span>}
          </IconButton>
        );
      })}
      {overflow.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button ref={more} variant="ghost" size="icon-sm" aria-label={`${overflow.length} more from plugins`} className="text-muted-foreground hover:text-foreground" />
            }
          >
            <EllipsisIcon aria-hidden />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {overflow.map((button) => {
              const Icon = panelIcons[button.icon] ?? GlobeIcon;
              return (
                <DropdownMenuItem
                  key={button.key}
                  onClick={() => props.onPluginButton(button, more.current?.getBoundingClientRect() ?? new DOMRect())}
                >
                  <Icon aria-hidden />
                  <span className="flex-1">{button.title}</span>
                  {button.label && <span className="text-done tabular-nums">{button.label}</span>}
                </DropdownMenuItem>
              );
            })}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <IconButton label="Past sessions" onClick={props.onHistory} disabled={!props.space}>
        <RotateCcwClockIcon />
      </IconButton>
      <IconButton label="Settings" onClick={props.onSettings}>
        <SettingsIcon />
      </IconButton>
      <IconButton label="Plugins" onClick={props.onPlugins}>
        <PuzzleIcon />
      </IconButton>
    </header>
  );
}
