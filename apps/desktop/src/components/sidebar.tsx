import { open } from "@tauri-apps/plugin-dialog";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { useState } from "react";
import { FolderIcon, FolderOpenIcon, PlusIcon, RefreshCwIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { GitCounts } from "@/components/git-counts";
import { PaneIcon } from "@/components/pane-label";
import { rollUp, StatusDot, stateLabel } from "@/components/status-dot";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { Agent, AgentState, GitStatus, Space } from "@/lib/api";
import { cn } from "@/lib/utils";

const WIDTH = 248;
const spring = { type: "spring", stiffness: 520, damping: 40, mass: 0.8 } as const;
const agentOrder: Record<AgentState, number> = { blocked: 0, done: 1, working: 2, idle: 3 };

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
    <motion.aside
      initial={false}
      animate={{ width: props.open ? WIDTH : 0, opacity: props.open ? 1 : 0 }}
      transition={spring}
      className="shrink-0 overflow-hidden"
      aria-hidden={!props.open}
    >
      <div className="flex h-full flex-col pb-2" style={{ width: WIDTH }}>
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
                <div className="flex flex-col items-start gap-2.5 px-2 py-3">
                  <p className="text-[13px] text-muted-foreground">Add a project folder to open a terminal in it.</p>
                  <motion.button
                    type="button"
                    whileTap={{ scale: 0.96 }}
                    onClick={() => void chooseFolder()}
                    className="inline-flex h-7 items-center gap-1.5 rounded-full bg-primary px-3 text-[12px] font-medium text-primary-foreground transition-colors duration-150 outline-none hover:bg-primary/85 focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <FolderOpenIcon className="size-3.5" aria-hidden />
                    Add project
                  </motion.button>
                </div>
              )}
              {spaces.map((space) => {
                const active = space.id === props.activeSpaceId;
                const spaceAgents = agents.filter((a) => a.spaceId === space.id);
                const state = rollUp(spaceAgents.map((a) => a.state));
                const status = props.git[space.id];
                const branch = status?.branch ?? space.branch;
                return (
                  <div key={space.id} className="group/row relative">
                    {active && (
                      <motion.span
                        layoutId="project-active"
                        transition={spring}
                        className="absolute inset-0 rounded-lg bg-sidebar-accent shadow-[inset_0_1px_0_rgb(255_255_255/0.04)]"
                      />
                    )}
                    <motion.button
                      type="button"
                      whileTap={{ scale: 0.985 }}
                      onClick={() => props.onSelectSpace(space.id)}
                      aria-current={active ? "page" : undefined}
                      title={space.path}
                      className={cn(
                        "relative flex w-full items-center gap-2.5 rounded-lg py-1.5 pr-8 pl-2 text-left outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring",
                        !active && "hover:bg-hover",
                      )}
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
                    </motion.button>
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
                  </div>
                );
              })}
            </nav>
          </LayoutGroup>
        </ScrollArea>

        <section aria-labelledby="agents-heading" className="mx-2 flex max-h-[46%] flex-col border-t border-hairline pt-2">
          <h2 id="agents-heading" className="flex items-center justify-between px-2 pb-1 text-[12px] font-medium text-muted-foreground">
            Agents
            {agents.length > 0 && <span className="tabular-nums">{agents.length}</span>}
          </h2>
          <ScrollArea className="min-h-0">
            <ul className="flex flex-col gap-px">
              {sortedAgents.length === 0 && (
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
                    <motion.button
                      type="button"
                      whileTap={{ scale: 0.985 }}
                      onClick={() => props.onSelectAgent(agent)}
                      className={cn(
                        "flex w-full items-center gap-2.5 rounded-lg py-1.5 pr-2 pl-2 text-left outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring",
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
                    </motion.button>
                  </motion.li>
                ))}
              </AnimatePresence>
            </ul>
          </ScrollArea>
        </section>
      </div>
    </motion.aside>
  );
}
