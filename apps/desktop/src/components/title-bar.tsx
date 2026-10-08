import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { HistoryIcon, PanelLeftIcon, PlusIcon, PuzzleIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { LayoutGlyph } from "@/components/layout-glyph";
import { PaneIcon, type PaneLabelInfo } from "@/components/pane-label";
import { StatusDot } from "@/components/status-dot";
import type { AgentState, Space } from "@/lib/api";
import type { LayoutNode } from "@/lib/layout";
import { isMac, shortcutLabel } from "@/lib/terminal";
import { cn } from "@/lib/utils";

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
}) {
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
        <div role="tablist" aria-label="Terminals" className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto">
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
                  <motion.button
                    type="button"
                    role="tab"
                    aria-selected={active}
                    whileTap={{ scale: 0.97 }}
                    transition={spring}
                    onClick={() => props.onSelect(tab.id)}
                    onAuxClick={(e) => e.button === 1 && props.onClose(tab.id)}
                    title={`${tab.label.title}  ${isMac ? "⌘" : "Ctrl+Shift+"}${i + 1}`}
                    className={cn(
                      "relative flex h-8 max-w-60 items-center gap-2 rounded-[10px] pr-8 pl-3 text-[13px] outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring",
                      active ? "text-foreground" : "text-muted-foreground hover:bg-hover hover:text-foreground",
                    )}
                  >
                    {tab.state && !tab.label.agent ? <StatusDot state={tab.state} /> : <PaneIcon agent={tab.label.agent} />}
                    <span className="truncate font-medium">{tab.label.title}</span>
                    {tab.label.detail && (
                      <span className="hidden truncate text-muted-foreground xl:inline">{tab.label.detail}</span>
                    )}
                    <LayoutGlyph layout={tab.layout} focused={tab.focusedPane} />
                  </motion.button>
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
          <IconButton label="New tab" shortcut={shortcutLabel.newTab} onClick={props.onNew} className="ml-1">
            <PlusIcon />
          </IconButton>
        </div>
      </LayoutGroup>

      <IconButton label="Past sessions" onClick={props.onHistory} disabled={!props.space}>
        <HistoryIcon />
      </IconButton>
      <IconButton label="Plugins" onClick={props.onPlugins}>
        <PuzzleIcon />
      </IconButton>
    </header>
  );
}
