import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { useEffect, useRef, useState, useSyncExternalStore, type ComponentProps } from "react";
import { flushSync } from "react-dom";
import {
  ArrowDownIcon,
  ArrowUpDownIcon,
  ArrowUpIcon,
  ChevronRightIcon,
  CircleAlertIcon,
  CircleDashedIcon,
  CopyIcon,
  FolderIcon,
  FileTextIcon,
  FolderOpenIcon,
  GitBranchIcon,
  GitBranchPlusIcon,
  GitForkIcon,
  WrenchIcon,
  RotateCcwClockIcon,
  SearchIcon,
  SettingsIcon,
  CircleArrowUpIcon,
  PinIcon,
  PinOffIcon,
  PlusIcon,
  RefreshCwIcon,
  SquareTerminalIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import { Item } from "@/components/ui/item";
import { NotificationBell } from "@/components/notification-bell";
import { RowHint, SidebarNavRow } from "@/components/sidebar-nav-row";
import { OpenInEditorItems } from "@/components/editor-menu";
import { GitCounts } from "@/components/git-counts";
import { agentLabel, PaneIcon } from "@/components/pane-label";
import { rollUp, StatusDot, stateLabel } from "@/components/status-dot";
import { Kbd } from "@/components/ui/kbd";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { elapsed } from "@/lib/agents";
import type { Agent, AgentKind, AgentState, GitStatus, PluginView, Space } from "@/lib/api";
import { ProjectSections, type OpenSectionRow } from "@/components/project-sections";
import type { ProjectSections as ProjectSectionsState } from "@/lib/project-sections";
import type { Phase } from "@/lib/sleep";
import { orderProjects, parseProjectSort, projectSorts, type ProjectRow, type ProjectSort } from "@/lib/project-order";
import { trackDrag } from "@/lib/pointer-drag";
import { availableUpdate, showUpdate } from "@/lib/updater";
import { canFork } from "@/lib/resume";
import { shortcutLabel } from "@/lib/terminal";
import { requestWorktreeAgent, requestWorktreeSetup, worktreeTitle } from "@/lib/worktree-agent";
import { cn } from "@/lib/utils";

const spring = { type: "spring", stiffness: 520, damping: 40, mass: 0.8 } as const;
const agentOrder: Record<AgentState, number> = { blocked: 0, done: 1, working: 2, idle: 3 };

const revealLabel = { macos: "Reveal in Finder", windows: "Show in Explorer" }[document.documentElement.dataset.platform ?? ""] ?? "Show in file manager";

const copy = (text: string) => void navigator.clipboard.writeText(text).catch(() => toast.error("Couldn't copy"));

function agentStatus(agent: Agent) {
  if (agent.state === "blocked" && agent.waitingFor) return `needs you: ${agent.waitingFor}`;
  return stateLabel[agent.state].toLowerCase();
}

/** " 42s" or " 7m": how long the agent has been working. Ticks every second for the first minute, then every 15 s. */
function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now);
  const fast = now - since < 60_000;
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), fast ? 1000 : 15_000);
    return () => clearInterval(id);
  }, [fast]);
  return <span className="tabular-nums"> {elapsed(now - since)}</span>;
}

export type PendingAgent = { key: string; spaceId: string; kind: AgentKind; phase: Phase; title: string | null };

const pendingLabel: Record<Phase, string> = {
  restored: "starts when you open it",
  sleeping: "sleeping",
  starting: "resuming…",
  failed: "couldn't resume, retry",
};

export function Sidebar(props: {
  open: boolean;
  /** The copy shown over the panes while the sidebar is closed, with its own row highlight. */
  floating?: boolean;
  spaces: Space[];
  git: Record<string, GitStatus>;
  onRefreshGit: () => Promise<unknown>;
  agents: Agent[];
  activeSpaceId: string | null;
  focusedPaneId: string | null;
  onSelectSpace: (id: string) => void;
  onSelectAgent: (agent: Agent) => void;
  /** Sessions with no live agent: restored ones you haven't opened yet, sleeping ones, ones being resumed and ones that couldn't be. */
  pending: PendingAgent[];
  /** Resumes a pending session (or retries one that failed) and goes to its pane. */
  onSelectPending: (key: string) => void;
  /** The pane key of each live agent's pane, by pane id, and each pane key's place in creation order. A row keeps its key and its place when the agent takes over from a pending row. */
  paneKeyOf: Record<string, string>;
  paneOrder: Record<string, number>;
  /** The name of the plugin that opened a pane, by pane key, for the badge on its Agents row. */
  openedBy: Record<string, string>;
  onNewTab: (spaceId: string) => void;
  /** Opens the project's Project page as a tab. */
  onOpenProjectPage: (spaceId: string) => void;
  /** What plugins list under each project, the plugins that are on, and the sections folded shut. */
  projectSections: ProjectSectionsState;
  sectionSorts: Record<string, string>;
  onSortSection: (pluginId: string, sectionId: string, sort: string) => void;
  plugins: PluginView[];
  collapsedSections: string[];
  onToggleSection: (projectId: string, key: string) => void;
  /** A row was clicked: its page opens as a tab in that project. */
  onOpenSectionRow: OpenSectionRow;
  onForkAgent: (agent: Agent) => void;
  onCloseAgent: (agent: Agent) => void;
  onAddSpace: (path: string) => void;
  onRemoveSpace: (id: string) => void;
  /** Moves a project to the place of the top-level project `slot` (worktrees follow their project). */
  onMoveSpace: (id: string, slot: string) => void;
  /** How projects are ordered; dragging reorders only in "manual". */
  sort: ProjectSort;
  onSortChange: (sort: ProjectSort) => void;
  /** When each project was last worked on, in ms. */
  lastActive: Record<string, number>;
  /** Whether the "Older" group (projects idle for 3 days) is open. */
  olderOpen: boolean;
  onToggleOlder: () => void;
  onTogglePin: (id: string, pinned: boolean) => void;
  /** The command palette. */
  onSearch: () => void;
  onHistory: () => void;
  onSettings: () => void;
  /** The Inbox row's popover: agent events, newest first. */
  notifications: Pick<ComponentProps<typeof NotificationBell>, "events" | "onOpen" | "onClear" | "onGoToPane" | "onButton">;
}) {
  const update = useSyncExternalStore(availableUpdate.subscribe, availableUpdate.get);
  const { spaces, agents } = props;
  const spaceName = (id: string) => spaces.find((s) => s.id === id)?.name ?? id;
  // One list, so a pending row and the live agent that replaces it sit in the same place under the same key.
  const agentRows = [
    ...agents.map((agent) => ({ id: props.paneKeyOf[agent.paneId] ?? agent.paneId, rank: agentOrder[agent.state], agent, pending: undefined })),
    ...props.pending.map((pending) => ({ id: pending.key, rank: agentOrder.idle, agent: undefined, pending })),
  ]
    .map((row) => ({ ...row, order: props.paneOrder[row.id] ?? Number.MAX_SAFE_INTEGER }))
    .sort((a, b) => a.rank - b.rank || a.order - b.order);
  const [refreshing, setRefreshing] = useState(false);
  const [dragId, setDragId] = useState<string | null>(null);
  const list = useRef<HTMLElement>(null);
  // A project with an agent (or a session waiting to resume) is never filed under Older.
  const busy = new Set([...agents.map((a) => a.spaceId), ...props.pending.map((p) => p.spaceId)]);
  const { shown, older } = orderProjects(spaces, { sort: props.sort, lastActive: props.lastActive, now: Date.now(), activeId: props.activeSpaceId, busy });
  // Only the listed projects take part in dragging and Move up/down; the ones under Older stay put.
  const tops = shown.filter((r) => !r.child).map((r) => r.space.id);
  const manual = props.sort === "manual";
  // A drag moves the project more than once, so each step reads the list as the last move left it.
  const topsNow = useRef(tops);
  topsNow.current = tops;

  /** Dragging a project up or down: it follows the pointer, and the others make room as it passes them. */
  function dragSpace(e: React.PointerEvent, id: string) {
    const inner = e.currentTarget.closest<HTMLElement>("[data-row-inner]");
    const outer = inner?.parentElement;
    if (!inner || !outer) return;
    const from = tops.indexOf(id);
    let at = from;
    let startTop = 0;
    const place = (dy: number) => {
      inner.style.transform = `translateY(${dy - (outer.getBoundingClientRect().top - startTop)}px)`;
    };
    trackDrag(e, {
      onStart: () => {
        flushSync(() => setDragId(id));
        startTop = outer.getBoundingClientRect().top;
        inner.style.zIndex = "10";
        document.body.style.cursor = "grabbing";
      },
      onMove: ({ dy }) => {
        place(dy);
        const box = inner.getBoundingClientRect();
        const centre = box.top + box.height / 2;
        const others = [...(list.current?.querySelectorAll<HTMLElement>("[data-row-top]") ?? [])].filter((el) => el !== outer);
        const next = others.filter((el) => {
          const r = el.getBoundingClientRect();
          return r.top + r.height / 2 < centre;
        }).length;
        if (next === at) return;
        at = next;
        // Committed now, so the slot is where it will stay before the offset is worked out again.
        flushSync(() => props.onMoveSpace(id, topsNow.current[next]));
        place(dy);
      },
      onEnd: (_, cancelled) => {
        if (cancelled && at !== from) flushSync(() => props.onMoveSpace(id, topsNow.current[from]));
        document.body.style.cursor = "";
        inner.style.transition = "transform 160ms cubic-bezier(0.22, 1, 0.36, 1)";
        inner.style.transform = "translateY(0)";
        setTimeout(() => {
          inner.style.transition = inner.style.transform = inner.style.zIndex = "";
          setDragId(null);
        }, 170);
      },
    });
  }

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

  function renderRow({ space, child }: ProjectRow<Space>, older = false) {
                const top = tops.indexOf(space.id);
                const active = space.id === props.activeSpaceId;
                const spaceAgents = agents.filter((a) => a.spaceId === space.id);
                const state = rollUp(spaceAgents.map((a) => a.state));
                const status = props.git[space.id];
                const branch = status?.branch ?? space.branch;
                // A worktree is titled by its branch when its folder only repeats it, so the branch isn't shown twice.
                const { title, subtitle } = child ? worktreeTitle(space.name, branch) : { title: space.name, subtitle: branch || null };
                const counts = status && (
                  <GitCounts
                    tinted
                    counts={[
                      { icon: "changed" as const, value: status.changed },
                      { icon: "push" as const, value: status.ahead },
                      { icon: "pull" as const, value: status.behind },
                    ].filter((c) => c.value > 0)}
                  />
                );
                return (
                  <ContextMenu key={space.id}>
                  <ContextMenuTrigger render={<motion.div data-row-top={child || older ? undefined : ""} layout={dragId === space.id ? false : "position"} transition={spring} className="group/row relative" />}>
                    <div data-row-inner className="relative">
                    {active && (
                      <motion.span
                        layoutId={dragId === space.id ? undefined : "project-active"}
                        transition={spring}
                        className="absolute inset-0 rounded-lg bg-sidebar-accent shadow-[inset_0_1px_0_rgb(255_255_255/0.04)]"
                      />
                    )}
                    <Item
                      size="xs"
                      render={<motion.button type="button" whileTap={{ scale: 0.985 }} />}
                      onClick={() => props.onSelectSpace(space.id)}
                      onPointerDown={child || older || !manual ? undefined : (e) => dragSpace(e, space.id)}
                      aria-current={active ? "page" : undefined}
                      title={space.path}
                      className={cn("relative flex-nowrap gap-2.5 py-[3px] pr-8 pl-2 text-left group-focus-within/row:pr-[3.75rem] group-hover/row:pr-[3.75rem] outline-none focus-visible:ring-2 focus-visible:ring-ring/50", child && "pl-6", !active && "hover:bg-hover")}
                    >
                      <span className="flex size-4 shrink-0 items-center justify-center">
                        {state ? <StatusDot state={state} /> : child ? <GitBranchIcon className="size-4 text-muted-foreground" /> : <FolderIcon className="size-4 text-muted-foreground" />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-2">
                          <span className={cn("min-w-0 flex-1 truncate text-[13px] leading-[18px]", active ? "font-semibold" : "font-medium")}>{title}</span>
                          {!subtitle && counts}
                        </span>
                        {subtitle && (
                          <span className="flex items-center gap-2">
                            <span className="min-w-0 flex-1 truncate text-[11px] leading-4 text-muted-foreground">{subtitle}</span>
                            {counts}
                          </span>
                        )}
                      </span>
                    </Item>
                    <span className="absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center">
                      {spaceAgents.length > 0 ? (
                        <span className="px-1 text-[11px] text-muted-foreground tabular-nums group-hover/row:hidden">
                          {spaceAgents.length}
                        </span>
                      ) : (
                        space.pinned && <PinIcon className="mx-1 size-3 text-muted-foreground/70 group-hover/row:hidden" aria-label="Pinned" />
                      )}
                      <IconButton
                        label="Project page"
                        side="right"
                        onClick={() => props.onOpenProjectPage(space.id)}
                        className="hidden size-6 group-hover/row:inline-flex focus-visible:inline-flex [&_svg]:size-3.5"
                      >
                        <FileTextIcon />
                      </IconButton>
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
                    {/* Issues, pull requests and schedules belong to the repo, which lists them above its worktrees; a worktree's own would repeat them. */}
                    {!child && (
                      <ProjectSections
                        projectId={space.id}
                        sections={props.projectSections[space.id]}
                        plugins={props.plugins}
                        collapsed={props.collapsedSections}
                        onToggle={props.onToggleSection}
                        onOpen={props.onOpenSectionRow}
                        sorts={props.sectionSorts}
                        onSort={props.onSortSection}
                      />
                    )}
                  </ContextMenuTrigger>
                  <ContextMenuContent>
                    <ContextMenuItem onClick={() => props.onNewTab(space.id)}>
                      <SquareTerminalIcon />
                      New tab
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => props.onOpenProjectPage(space.id)}>
                      <FileTextIcon />
                      Project page
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => requestWorktreeAgent(space.id)}>
                      <GitBranchPlusIcon />
                      New agent in worktree…
                    </ContextMenuItem>
                    <ContextMenuItem onClick={() => requestWorktreeSetup(space.id)}>
                      <WrenchIcon />
                      Worktree setup…
                    </ContextMenuItem>
                    <OpenInEditorItems spaceId={space.id} />
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
                    {!child && (
                      <>
                        <ContextMenuSeparator />
                        <ContextMenuItem onClick={() => props.onTogglePin(space.id, !space.pinned)}>
                          {space.pinned ? <PinOffIcon /> : <PinIcon />}
                          {space.pinned ? "Unpin" : "Pin to top"}
                        </ContextMenuItem>
                        {manual && !older && (
                          <>
                            <ContextMenuItem disabled={top === 0} onClick={() => props.onMoveSpace(space.id, tops[top - 1])}>
                              <ArrowUpIcon />
                              Move up
                            </ContextMenuItem>
                            <ContextMenuItem disabled={top === tops.length - 1} onClick={() => props.onMoveSpace(space.id, tops[top + 1])}>
                              <ArrowDownIcon />
                              Move down
                            </ContextMenuItem>
                          </>
                        )}
                      </>
                    )}
                    <ContextMenuSeparator />
                    <ContextMenuItem variant="destructive" onClick={() => props.onRemoveSpace(space.id)}>
                      <Trash2Icon />
                      Remove from sidebar
                    </ContextMenuItem>
                  </ContextMenuContent>
                  </ContextMenu>
                );
  }

  return (
    <aside className={cn("h-full transition-opacity duration-200", !props.open && "opacity-0")} aria-hidden={!props.open}>
      {/* At least the narrowest width, so closing clips the list rather than squeezing it. */}
      <div className="flex h-full min-w-[200px] flex-col pb-2">
        <nav aria-label="Wings" className="flex shrink-0 flex-col gap-px px-2 pb-2">
          <SidebarNavRow icon={<SearchIcon />} label="Search" trailing={<RowHint>{shortcutLabel.palette}</RowHint>} onClick={props.onSearch} />
          {/* Only the docked copy listens for the show-notifications command, so the peeking copy doesn't open a second popover. */}
          <NotificationBell {...props.notifications} listen={!props.floating} />
          <SidebarNavRow icon={<RotateCcwClockIcon />} label="Past sessions" onClick={props.onHistory} disabled={props.activeSpaceId === null} />
        </nav>

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
            <DropdownMenu>
              <DropdownMenuTrigger
                render={<Button variant="ghost" size="icon-sm" aria-label="Sort projects" title="Sort projects" className="size-6 text-muted-foreground hover:bg-hover hover:text-foreground [&_svg]:size-3.5" />}
              >
                <ArrowUpDownIcon aria-hidden />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuRadioGroup value={props.sort} onValueChange={(v) => props.onSortChange(parseProjectSort(v))}>
                  {/* Base UI throws for a group label outside its group. */}
                  <DropdownMenuLabel>Sort projects</DropdownMenuLabel>
                  {projectSorts.map((m) => (
                    <DropdownMenuRadioItem key={m.value} value={m.value}>
                      {m.label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            <IconButton label="Add project" side="right" onClick={() => void chooseFolder()} className="size-6">
              <PlusIcon />
            </IconButton>
          </span>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <LayoutGroup id={props.floating ? "projects-floating" : "projects"}>
            <nav ref={list} aria-label="Projects" className="flex flex-col gap-px px-2 pb-2">
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
              {shown.map((row) => renderRow(row))}
              {older.length > 0 && (
                <>
                  <button
                    type="button"
                    aria-expanded={props.olderOpen}
                    aria-controls="older-projects"
                    title={`Older, ${older.filter((r) => !r.child).length}`}
                    onClick={props.onToggleOlder}
                    className="mt-1 flex h-6 w-full items-center gap-1 rounded-md px-2 text-left text-[11px] text-muted-foreground outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring/50"
                  >
                    <ChevronRightIcon className={cn("size-3 shrink-0 transition-transform duration-150 motion-reduce:transition-none", props.olderOpen && "rotate-90")} aria-hidden />
                    <span className="truncate">Older</span>
                    <span className="tabular-nums">{older.filter((r) => !r.child).length}</span>
                  </button>
                  {props.olderOpen && <div id="older-projects" className="flex flex-col gap-px">{older.map((row) => renderRow(row, true))}</div>}
                </>
              )}
            </nav>
          </LayoutGroup>
        </ScrollArea>

        <section aria-labelledby="agents-heading" className="mx-2 flex shrink-0 flex-col border-t border-hairline pt-2">
          <h2 id="agents-heading" className="flex items-center justify-between px-2 pb-1 text-[12px] font-medium text-muted-foreground">
            <Tooltip>
              <TooltipTrigger render={<span tabIndex={0} className="cursor-default rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />}>
                Agents
              </TooltipTrigger>
              <TooltipContent side="right" className="gap-2">
                Next agent that needs you
                <Kbd>{shortcutLabel.nextAgent}</Kbd>
              </TooltipContent>
            </Tooltip>
            {agentRows.length > 0 && <span className="tabular-nums">{agentRows.length}</span>}
          </h2>
          {/* Grows with the list up to five agents, then scrolls. */}
          <ScrollArea className="max-h-[249px] min-h-0">
            <ul className="flex flex-col gap-px">
              {agentRows.length === 0 && (
                <li className="px-2 py-1.5 text-[13px] text-muted-foreground">
                  Run <code className="rounded bg-hover px-1 py-px text-[12px]">claude</code> in any terminal and it shows up here.
                </li>
              )}
              <AnimatePresence initial={false}>
                {agentRows.map(({ id, agent, pending }) =>
                  pending ? (
                    <motion.li key={id} layout initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={spring}>
                      <Item
                        size="xs"
                        render={<motion.button type="button" whileTap={{ scale: 0.985 }} />}
                        onClick={() => props.onSelectPending(pending.key)}
                        aria-busy={pending.phase === "starting"}
                        className={cn("flex-nowrap gap-2.5 px-2 py-1.5 text-left", pending.phase === "starting" ? "cursor-progress" : "hover:bg-hover")}
                      >
                        <span className="flex size-4 shrink-0 items-center justify-center">
                          {pending.phase === "starting" ? (
                            <span className="size-2.5 rounded-full border-[1.5px] border-working/30 border-t-working motion-safe:animate-spin" aria-hidden />
                          ) : pending.phase === "failed" ? (
                            <CircleAlertIcon className="size-3.5 text-blocked" aria-hidden />
                          ) : (
                            <CircleDashedIcon className="size-3.5 text-muted-foreground" aria-hidden />
                          )}
                        </span>
                        <span className="min-w-0 flex-1" title={`${pending.title ?? agentLabel(pending.kind)}\n${spaceName(pending.spaceId)}, ${pendingLabel[pending.phase]}`}>
                          <span className={cn("block truncate text-[13px] font-medium", pending.phase !== "starting" && "text-muted-foreground")}>{pending.title ?? agentLabel(pending.kind)}</span>
                          <span className="block truncate text-[11px] leading-4 text-muted-foreground">
                            {spaceName(pending.spaceId)}, {pendingLabel[pending.phase]}
                          </span>
                        </span>
                      </Item>
                    </motion.li>
                  ) : agent && (
                  <motion.li
                    key={id}
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
                      <span className="min-w-0 flex-1" title={`${agent.name ?? agentLabel(agent.kind)}\n${spaceName(agent.spaceId)}, ${agentStatus(agent)}${agent.summary ? `\n${agent.summary}` : ""}`}>
                        <span className="flex items-center gap-1.5">
                          <span className="min-w-0 truncate text-[13px] font-medium">{agent.name ?? agentLabel(agent.kind)}</span>
                          {props.openedBy[id] && (
                            <Badge variant="secondary" className="h-4 shrink-0 px-1.5 text-[10px]" title={`Started by the ${props.openedBy[id]} plugin`}>
                              {props.openedBy[id]}
                            </Badge>
                          )}
                        </span>
                        <span className="block truncate text-[11px] leading-4 text-muted-foreground">
                          {spaceName(agent.spaceId)}, {agentStatus(agent)}
                          {agent.state === "working" && agent.stateSince && <Elapsed since={agent.stateSince} />}
                        </span>
                        {agent.summary && <span className="block truncate text-[11px] leading-4 text-muted-foreground/70">{agent.summary}</span>}
                      </span>
                      {agent.state === "done" && <span className="size-1.5 shrink-0 rounded-full bg-done" aria-hidden />}
                    </Item>
                      </ContextMenuTrigger>
                      <ContextMenuContent>
                        <ContextMenuItem onClick={() => props.onSelectAgent(agent)}>
                          <SquareTerminalIcon />
                          Go to pane
                        </ContextMenuItem>
                        <ContextMenuItem disabled={!canFork(agent.kind) || !agent.sessionId} onClick={() => props.onForkAgent(agent)}>
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
                  ),
                )}
              </AnimatePresence>
            </ul>
          </ScrollArea>
        </section>
        <div className="mx-2 mt-1 shrink-0">
          {update && (
            <SidebarNavRow
              icon={<CircleArrowUpIcon className="text-done" />}
              label={`Update to Wings ${update.version}`}
              title="See what's new and install it"
              onClick={() => showUpdate(update)}
              className="text-foreground"
            />
          )}
          <SidebarNavRow icon={<SettingsIcon />} label="Settings" trailing={<RowHint>{shortcutLabel.settings}</RowHint>} onClick={props.onSettings} />
        </div>
      </div>
    </aside>
  );
}
