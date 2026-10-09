import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { useState } from "react";
import {
  CircleDashedIcon,
  CopyIcon,
  FolderIcon,
  FolderOpenIcon,
  GitForkIcon,
  PlusIcon,
  RefreshCwIcon,
  SquareTerminalIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { buttonVariants } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import { Item } from "@/components/ui/item";
import { GitCounts } from "@/components/git-counts";
import { PaneIcon } from "@/components/pane-label";
import { rollUp, StatusDot, stateLabel } from "@/components/status-dot";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { Agent, AgentState, GitStatus, Space } from "@/lib/api";
import { cn } from "@/lib/utils";

const spring = { type: "spring", stiffness: 520, damping: 40, mass: 0.8 } as const;
const agentOrder: Record<AgentState, number> = { blocked: 0, done: 1, working: 2, idle: 3 };

const revealLabel = { macos: "Reveal in Finder", windows: "Show in Explorer" }[document.documentElement.dataset.platform ?? ""] ?? "Show in file manager";

const copy = (text: string) => void navigator.clipboard.writeText(text).catch(() => {});

function agentStatus(agent: Agent) {
  if (agent.state === "blocked" && agent.waitingFor) return `needs you: ${agent.waitingFor}`;
  return stateLabel[agent.state].toLowerCase();
}

export function Sidebar(props: {
  open: boolean;
  spaces: Space[];
  git: Record<string, GitStatus>;
  onRefreshGit: () => Promise<unknown>;
  agents: Agent[];
  activeSpaceId: string | null;
  focusedPaneId: string | null;
  onSelectSpace: (id: string) => void;
  onSelectAgent: (agent: Agent) => void;
  /** Claude sessions in tabs Wings restored but you haven't opened yet: they start when you go to them. */
  restored: { key: string; spaceId: string; title: string | null }[];
  onSelectRestored: (key: string) => void;
  onNewTab: (spaceId: string) => void;
  onForkAgent: (agent: Agent) => void;
  onCloseAgent: (agent: Agent) => void;
  onAddSpace: (path: string) => void;
  onRemoveSpace: (id: string) => void;
}) {
  const { spaces, agents } = props;
  const spaceName = (id: string) => spaces.find((s) => s.id === id)?.name ?? id;
  const sortedAgents = [...agents].sort((a, b) => agentOrder[a.state] - agentOrder[b.state]);
  const [refreshing, setRefreshing] = useState(false);

  async function refreshGit() {
    setRefreshing(true);
    try {
      await props.onRefreshGit();
    } finally {
      setRefreshing(false);
    }
  }

  async function chooseFolder() {
    const path = await open({ directory: true, multiple: false, title: "Add a project to Wings" });
    if (typeof path === "string") props.onAddSpace(path);
  }

  return (
    <aside className={cn("h-full transition-opacity duration-200", !props.open && "opacity-0")} aria-hidden={!props.open}>
      {/* At least the narrowest width, so closing clips the list rather than squeezing it. */}
      <div className="flex h-full min-w-[200px] flex-col pb-2">
        <div className="flex h-8 items-center justify-between pr-2 pl-4">
          <h2 className="text-[12px] font-medium text-muted-foreground">Projects</h2>
          <span className="flex items-center">
            <IconButton
              label={refreshing ? "Checking git status…" : "Refresh git status"}
              side="right"
              onClick={() => void refreshGit()}
              disabled={refreshing}
              className="size-6 [&_svg]:size-3.5"
            >
              <RefreshCwIcon className={cn(refreshing && "motion-safe:animate-spin")} />
            </IconButton>
            <IconButton label="Add project" side="right" onClick={() => void chooseFolder()} className="size-6">
              <PlusIcon />
            </IconButton>
          </span>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <LayoutGroup id="projects">
            <nav aria-label="Projects" className="flex flex-col gap-px px-2 pb-2">
              {spaces.length === 0 && (
                <Empty className="items-start p-2 text-left">
                  <EmptyHeader className="items-start text-left">
                    <EmptyDescription className="text-[13px]">Add a project folder to open a terminal in it.</EmptyDescription>
                  </EmptyHeader>
                  <EmptyContent className="items-start">
                    <motion.button
                      type="button"
                      whileTap={{ scale: 0.96 }}
                      onClick={() => void chooseFolder()}
                      className={cn(buttonVariants({ size: "sm" }), "rounded-full px-3 text-[12px]")}
                    >
                      <FolderOpenIcon aria-hidden />
                      Add project
                    </motion.button>
                  </EmptyContent>
                </Empty>
              )}
              {spaces.map((space) => {
                const active = space.id === props.activeSpaceId;
                const spaceAgents = agents.filter((a) => a.spaceId === space.id);
                const state = rollUp(spaceAgents.map((a) => a.state));
                const status = props.git[space.id];
                const branch = status?.branch ?? space.branch;
                return (
                  <ContextMenu key={space.id}>
                  <ContextMenuTrigger render={<div className="group/row relative" />}>
                    {active && (
                      <motion.span
                        layoutId="project-active"
                        transition={spring}
                        className="absolute inset-0 rounded-lg bg-sidebar-accent shadow-[inset_0_1px_0_rgb(255_255_255/0.04)]"
                      />
                    )}
                    <Item
                      size="xs"
                      render={<motion.button type="button" whileTap={{ scale: 0.985 }} />}
                      onClick={() => props.onSelectSpace(space.id)}
                      aria-current={active ? "page" : undefined}
                      title={space.path}
                      className={cn("relative flex-nowrap gap-2.5 py-1.5 pr-8 pl-2 text-left", !active && "hover:bg-hover")}
                    >
                      <span className="flex size-4 shrink-0 items-center justify-center">
                        {state ? <StatusDot state={state} /> : <FolderIcon className="size-4 text-muted-foreground" />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className={cn("block truncate text-[13px]", active ? "font-semibold" : "font-medium")}>
                          {space.name}
                        </span>
                        {branch && (
                          <span className="flex items-center gap-2">
                            <span className="min-w-0 flex-1 truncate text-[11px] leading-4 text-muted-foreground">{branch}</span>
                            {status && (
                              <GitCounts
                                tinted
                                counts={[
                                  { icon: "changed" as const, value: status.changed },
                                  { icon: "push" as const, value: status.ahead },
                                  { icon: "pull" as const, value: status.behind },
                                ].filter((c) => c.value > 0)}
                              />
                            )}
                          </span>
                        )}
                      </span>
                    </Item>
                    <span className="absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center">
                      {spaceAgents.length > 0 && (
                        <span className="px-1 text-[11px] text-muted-foreground tabular-nums group-hover/row:hidden">
                          {spaceAgents.length}
                        </span>
                      )}
                      <IconButton
                        label="Remove from sidebar"
                        side="right"
                        onClick={() => props.onRemoveSpace(space.id)}
                        className="hidden size-6 group-hover/row:inline-flex focus-visible:inline-flex [&_svg]:size-3.5"
                      >
                        <XIcon />
                      </IconButton>
                    </span>
                  </ContextMenuTrigger>
                  <ContextMenuContent>
                    <ContextMenuItem onClick={() => props.onNewTab(space.id)}>
                      <SquareTerminalIcon />
                      New tab
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => void revealItemInDir(space.path)}>
                      <FolderOpenIcon />
                      {revealLabel}
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => copy(space.path)}>
                      <CopyIcon />
                      Copy path
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => void refreshGit()} disabled={refreshing}>
                      <RefreshCwIcon />
                      Refresh git status
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem variant="destructive" onClick={() => props.onRemoveSpace(space.id)}>
                      <Trash2Icon />
                      Remove from sidebar
                    </ContextMenuItem>
                  </ContextMenuContent>
                  </ContextMenu>
                );
              })}
            </nav>
          </LayoutGroup>
        </ScrollArea>

        <section aria-labelledby="agents-heading" className="mx-2 flex shrink-0 flex-col border-t border-hairline pt-2">
          <h2 id="agents-heading" className="flex items-center justify-between px-2 pb-1 text-[12px] font-medium text-muted-foreground">
            Agents
            {agents.length + props.restored.length > 0 && <span className="tabular-nums">{agents.length + props.restored.length}</span>}
          </h2>
          {/* Grows with the list up to five agents, then scrolls. */}
          <ScrollArea className="max-h-[249px] min-h-0">
            <ul className="flex flex-col gap-px">
              {sortedAgents.length === 0 && props.restored.length === 0 && (
                <li className="px-2 py-1.5 text-[13px] text-muted-foreground">
                  Run <code className="rounded bg-hover px-1 py-px text-[12px]">claude</code> in any terminal and it shows up here.
                </li>
              )}
              <AnimatePresence initial={false}>
                {sortedAgents.map((agent) => (
                  <motion.li
                    key={agent.paneId}
                    layout
                    initial={{ opacity: 0, y: -6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -6 }}
                    transition={spring}
                  >
                    <ContextMenu>
                      <ContextMenuTrigger render={<div />}>
                    <Item
                      size="xs"
                      render={<motion.button type="button" whileTap={{ scale: 0.985 }} />}
                      onClick={() => props.onSelectAgent(agent)}
                      className={cn(
                        "flex-nowrap gap-2.5 px-2 py-1.5 text-left",
                        agent.paneId === props.focusedPaneId ? "bg-sidebar-accent" : "hover:bg-hover",
                        agent.state === "blocked" && "bg-blocked/10 hover:bg-blocked/15",
                      )}
                    >
                      <span className="flex size-4 shrink-0 items-center justify-center">
                        <PaneIcon agent={agent} />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium">{agent.name ?? "Claude"}</span>
                        <span className="block truncate text-[11px] leading-4 text-muted-foreground">
                          {spaceName(agent.spaceId)}, {agentStatus(agent)}
                        </span>
                      </span>
                      {agent.state === "done" && <span className="size-1.5 shrink-0 rounded-full bg-done" aria-hidden />}
                    </Item>
                      </ContextMenuTrigger>
                      <ContextMenuContent>
                        <ContextMenuItem onClick={() => props.onSelectAgent(agent)}>
                          <SquareTerminalIcon />
                          Go to pane
                        </ContextMenuItem>
                        <ContextMenuItem disabled={!agent.sessionId} onClick={() => props.onForkAgent(agent)}>
                          <GitForkIcon />
                          Fork in new tab
                        </ContextMenuItem>
                        <ContextMenuItem disabled={!agent.sessionId} onClick={() => agent.sessionId && copy(agent.sessionId)}>
                          <CopyIcon />
                          Copy session id
                        </ContextMenuItem>
                        <ContextMenuSeparator />
                        <ContextMenuItem variant="destructive" onClick={() => props.onCloseAgent(agent)}>
                          <XIcon />
                          Close pane
                        </ContextMenuItem>
                      </ContextMenuContent>
                    </ContextMenu>
                  </motion.li>
                ))}
                {props.restored.map((r) => (
                  <motion.li key={r.key} layout initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={spring}>
                    <Item
                      size="xs"
                      render={<motion.button type="button" whileTap={{ scale: 0.985 }} />}
                      onClick={() => props.onSelectRestored(r.key)}
                      className="flex-nowrap gap-2.5 px-2 py-1.5 text-left hover:bg-hover"
                    >
                      <span className="flex size-4 shrink-0 items-center justify-center">
                        <CircleDashedIcon className="size-3.5 text-muted-foreground" aria-hidden />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium text-muted-foreground">{r.title ?? "Claude"}</span>
                        <span className="block truncate text-[11px] leading-4 text-muted-foreground">{spaceName(r.spaceId)}, starts when you open it</span>
                      </span>
                    </Item>
                  </motion.li>
                ))}
              </AnimatePresence>
            </ul>
          </ScrollArea>
        </section>
      </div>
    </aside>
  );
}
