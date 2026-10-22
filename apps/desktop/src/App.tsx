import { getCurrentWebview } from "@tauri-apps/api/webview";
import { usePanelRef, type PanelImperativeHandle } from "react-resizable-panels";
import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, type ComponentProps } from "react";
import { toast } from "sonner";
import { MotionConfig } from "motion/react";

import { CommandPalette } from "@/components/command-palette";
import { DiffViewer } from "@/components/diff-viewer";
import { notificationsReducer, restoreNotifications, type NotificationEvent } from "@/lib/notifications";
import { reviewTargets, sendToPane } from "@/lib/review";
import { HistorySheet } from "@/components/history-sheet";
import { PluginModal, type ModalRef } from "@/components/plugin-modal";
import { Welcome } from "@/components/welcome";
import { RightSidebar, type SidebarRef } from "@/components/right-sidebar";
import { PaneGrid, type PaneActions } from "@/components/pane-grid";
import type { PaneRailEntry } from "@/components/pane-rail";
import type { PaneStatusEntry } from "@/components/pane-status";
import { agentLabel, paneLabel, type PaneLabelInfo } from "@/components/pane-label";
import { Sidebar } from "@/components/sidebar";
import { SidebarPeek } from "@/components/sidebar-peek";
import { rollUp, StatusDot } from "@/components/status-dot";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { SettingsDialog, type SettingsSection } from "@/components/settings-dialog";
import { ToolInstallDialog } from "@/components/tool-install";
import { pluginsUsing } from "@/lib/tool-install";
import { usePluginUpdates } from "@/lib/plugin-updates";
import { SIDEBAR_UNLOAD_MS, TAB_PAGE_UNLOAD_MS } from "@/lib/idle-unload";
import { useIdleUnload } from "@/lib/use-idle-unload";
import { newTabAction, firstTabFor, parseNewProjectOpens, type NewProjectOpens, isProjectTab, isViewTab, newTabEntries, openProjectTab, openViewTab, pluginSettingsSection, restoreTabs, saveTabs, settingsPlugins, shownSection, staleViewTabs, type NewTabEntry, type ProjectTab, type SavedProjectTab, type SavedViewTab, type ViewTab } from "@/lib/plugin-pages";
import { parseProjectSort, restoreLastActive, touched, withMissing, type ProjectSort } from "@/lib/project-order";
import { collapseKey, restoreCollapsed, toggleCollapsed, withoutPlugin, withoutProject, withoutProjectCollapsed, withSection, type ProjectSections } from "@/lib/project-sections";
import { PluginTab } from "@/components/plugin-tab";
import { ProjectPageTab } from "@/components/project-page/project-page-tab";
import { PROJECT_PAGE_FIND } from "@/components/project-page/search";
import { TerminalCommandPrompt } from "@/components/terminal-command";
import { ProjectBar, TitleBar, type PluginButton, type TabView } from "@/components/title-bar";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Toaster } from "@/components/ui/sonner";
import { cleanSound, DEFAULT_SOUND, notificationSound, setSoundSettings, type SoundSettings } from "@/lib/sound";
import { TooltipProvider } from "@/components/ui/tooltip";
import { api, pagesOf, type Agent, type AgentState, type GitStatus, type LivePane, type PaneInfo, type PluginView, type Space } from "@/lib/api";
import { PluginHost, type Badge, type DiffView, type PaneRail, type PaneStatus, type SidebarLabel } from "@/lib/plugins";
import { withBadge, withLabel } from "@/lib/badges";
import { findPane, insertPane, mapPanes, pane, paneIds, paneNames, remove, renamePane, setRatio, split, type LayoutNode } from "@/lib/layout";
import { moveTab, reorder, sortByOrder } from "@/lib/tab-order";
import { tabTitles } from "@/lib/tab-names";
import { requestWorktreeAgent } from "@/lib/worktree-agent";
import { nextNeedingYou } from "@/lib/agents";
import { createCommands, shortcutOf, shortcutText } from "@/lib/commands";
import { canGoBack, canGoForward, EMPTY_NAV, goBack, goForward, visit, type NavState, type Place } from "@/lib/nav-history";
import { defaultEditor, openInEditor, useDefaultEditorId, useEditors } from "@/lib/editors";
import { DEFAULT_GLOBAL_HOTKEY, hotkeyLabel, type GlobalHotkey } from "@/lib/hotkey";
import type { PaletteItem } from "@/lib/palette";
import { openNotifications, openReviewSend } from "@/lib/ui-events";
import { stateLabel } from "@/components/status-dot";
import { DEFAULT_FONT_SIZE, setTerminalFontSize, isMac, shortcutFor, terminals, TerminalSession, DEFAULT_TERMINAL_KEYS, setTerminalKeys, type TerminalKeys, DEFAULT_TERMINAL_LOOK, setTerminalLook, type TerminalLook } from "@/lib/terminal";
import { forkLine, isSessionIdOf, isShellCommand, paneCwds, resumeLine, shellQuote, type SessionKind } from "@/lib/resume";
import { paneInput } from "@/lib/pane-input";
import { canSleep, DEFAULT_SLEEP, EXIT_GRACE_MS, nextResumeDeadline, noResume, phaseOf, planSleep, resumeExpired, settled, type SleepSettings } from "@/lib/sleep";
import { UpdateDialog } from "@/components/update-dialog";
import { WorktreeAgentDialog } from "@/components/worktree-agent-dialog";
import { checkForUpdate, startUpdateChecks } from "@/lib/updater";

/** `name` is what you called the tab; absent means its automatic title. */
type PaneTab = { id: string; spaceId: string; layout: LayoutNode; focusedPane: string; zoomedPane: string | null; name?: string };
/** A tab of terminal panes, or a plugin view open as a tab. */
type Tab = PaneTab | ViewTab | ProjectTab;
const isPaneTab = (tab: Tab): tab is PaneTab => "layout" in tab;
/** The terminal tab holding a pane. */
const tabOfPane = (tabs: Tab[], key: string) => tabs.find((t): t is PaneTab => isPaneTab(t) && paneIds(t.layout).includes(key));
/**
 * `sessionId` is the agent session last seen in the pane, of agent `kind`, `name` its title; `live` once Wings has seen
 * it running this launch; `args` its flags. `asleep`, `resumeAt` and `resumeFailed` are the sleep and resume state (`sleep.ts`).
 * `savedCwd` is the folder it was in when Wings last saved, until its shell reports one. `historyId` names the output
 * Wings keeps for it across a restart.
 */
type PaneMeta = { spaceId: string; paneId: string | null; sessionId: string | null; kind: SessionKind; name: string | null; live: boolean; args: string[]; asleep: boolean; resumeAt: number | null; resumeFailed: boolean; savedCwd?: string | null; historyId: string };

type SavedWorkspace = {
  version: 1;
  activeSpaceId: string | null;
  activeTabBySpace: Record<string, string>;
  /** Project ids in the order you dragged them into. Absent in older files. */
  spaceOrder?: string[];
  /** How the sidebar orders projects (absent is manual), whether its "Older" group is open, and when each project was last worked on, in ms. Absent in older files. */
  projectSort?: ProjectSort;
  olderOpen?: boolean;
  lastActive?: Record<string, number>;
  sidebarOpen: boolean;
  /** Sidebar widths in pixels, as last dragged. */
  widths?: Widths;
  /** Plugin title bar buttons you pinned, by key. */
  pinnedButtons?: string[];
  fontSize: number;
  /** The terminal's keyboard settings. */
  terminalKeys?: TerminalKeys;
  /** The shortcut that brings Wings forward from any app. */
  globalHotkey?: GlobalHotkey;
  /** The terminal's font, cursor, scrollback, copy and contrast settings. */
  terminalLook?: TerminalLook;
  tabs: PaneTab[];
  /** Plugin views open as tabs, apart from `tabs` so an older Wings still loads those. Absent in older files. */
  viewTabs?: SavedViewTab[];
  /** Project pages open as tabs, apart from `tabs` for the same reason. Absent in older files. */
  projectTabs?: SavedProjectTab[];
  /** What the + button opens, as `pluginId:viewId`; absent or null is a terminal. */
  newTabPage?: string | null;
  /** What a project added to Wings opens first: "terminal" (absent) or "project" for its Project page. */
  newProjectOpens?: NewProjectOpens;
  /** Plugin sections folded shut under a project, as `collapseKey` makes them. Absent in older files. */
  collapsedSections?: string[];
  /** The Inbox. Absent in older files. */
  notifications?: NotificationEvent[];
  sessions: Record<string, string | null>;
  /** Which agent each session in `sessions` belongs to. Absent in older files, which held Claude sessions only. */
  sessionKinds?: Record<string, SessionKind>;
  /** Panes whose agent Wings put to sleep. */
  asleep?: string[];
  /** When idle agents sleep. */
  sleep?: SleepSettings;
  /** The sound for new notifications. */
  notifySound?: SoundSettings;
  /** Whether Claude's `open_pane` may press Enter after the command it types. */
  agentsRunCommands?: boolean;
  /** Flags to resume each pane's session with, by the same keys as `sessions`. Absent in older files. */
  sessionArgs?: Record<string, string[]>;
  /** The folder each pane's shell was in, by the same keys as `sessions`. Absent in older files. */
  cwds?: Record<string, string>;
  /** The Rust pane behind each pane, by the same keys. Still running only after a webview reload. */
  paneIds?: Record<string, string>;
  /** Each pane's `historyId`. Absent in older files. */
  histories?: Record<string, string>;
};

function parseSaved(json: string | null): SavedWorkspace | null {
  try {
    const saved = json ? JSON.parse(json) : null;
    return saved?.version === 1 ? saved : null;
  } catch {
    return null;
  }
}

/** Saved panes whose Rust pane still runs in the same project, as saved key to pane id. Only after a webview reload. */
function stillRunning(saved: SavedWorkspace | null, live: LivePane[], spaces: Space[]) {
  const running = new Map<string, string>();
  for (const tab of saved?.tabs ?? []) {
    if (!spaces.some((s) => s.id === tab.spaceId)) continue;
    for (const key of paneIds(tab.layout)) {
      const id = saved?.paneIds?.[key];
      if (id && live.some((p) => p.id === id && p.spaceId === tab.spaceId)) running.set(key, id);
    }
  }
  return running;
}

type Widths = { left: number; right: number };

/** Sidebar sizes in pixels. The right one closes once it's dragged below its minimum. */
const SIDES = { left: { min: 200, max: 420, initial: 248 }, right: { min: 250, max: 640, initial: 360 } } as const;
/** A saved width out of range, like a near-zero one WebKit can report while a panel collapses, falls back. */
const fit = (side: keyof Widths, width: number) => (width >= SIDES[side].min && width <= SIDES[side].max ? width : SIDES[side].initial);

/** The library gives each panel an inline `overflow: auto`, so content that briefly overflows while it animates,
 * like a new tab's panes, flashed scrollbars. Panes and sidebars scroll inside themselves. */
const PANEL_STYLE = { overflow: "hidden" } as const;

/** Opens a side panel at its saved width, or closes it. */
function place(panel: PanelImperativeHandle | null, open: boolean, width: number, force: boolean) {
  if (!panel) return;
  if (!open) return void (panel.isCollapsed() || panel.collapse());
  if (force || panel.isCollapsed()) panel.resize(width);
}

/** The pane next to `key` in a direction, by on-screen position (cards carry `data-pane-key`). */
function neighborPane(key: string, dir: "left" | "right" | "up" | "down") {
  const card = (k: string) => document.querySelector<HTMLElement>(`[data-pane-key="${k}"]`);
  const from = card(key)?.getBoundingClientRect();
  if (!from) return null;
  const grid = card(key)?.closest("[data-pane-grid]");
  let best: { key: string; score: number } | null = null;
  for (const el of grid?.querySelectorAll<HTMLElement>("[data-pane-key]") ?? []) {
    const r = el.getBoundingClientRect();
    const gap = { left: from.left - r.right, right: r.left - from.right, up: from.top - r.bottom, down: r.top - from.bottom }[dir];
    if (gap < -1) continue;
    const across = dir === "left" || dir === "right" ? Math.abs(r.top - from.top) : Math.abs(r.left - from.left);
    const score = gap + across / 4;
    if (el.dataset.paneKey !== key && (!best || score < best.score)) best = { key: el.dataset.paneKey!, score };
  }
  return best?.key ?? null;
}

export default function App() {
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [paneInfo, setPaneInfo] = useState<Record<string, PaneInfo>>({});
  const [git, setGit] = useState<Record<string, GitStatus>>({});
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [panes, setPanes] = useState<Record<string, PaneMeta>>({});
  const [initialInputs, setInitialInputs] = useState<Record<string, string | null>>({});
  const [activeSpaceId, setActiveSpaceId] = useState<string | null>(null);
  const [activeTabBySpace, setActiveTabBySpace] = useState<Record<string, string>>({});
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [pinnedButtons, setPinnedButtons] = useState<string[]>([]);
  const [widths, setWidths] = useState<Widths>({ left: SIDES.left.initial, right: SIDES.right.initial });
  const frame = useRef<HTMLDivElement>(null);
  const mainBox = useRef<HTMLDivElement>(null);
  // The title bar's tabs line up with the main pane. Its edges go to CSS variables straight from a ResizeObserver, so
  // dragging or sliding a sidebar moves the tabs without rendering App.
  // Before the first paint, so the tabs don't jump from their start position to the sidebar's edge on launch.
  useLayoutEffect(() => {
    const box = mainBox.current;
    // Only the title bar reads them, so they go on it: a variable on the root would restyle every pane on each change.
    const root = frame.current?.querySelector<HTMLElement>("[data-title-bar]");
    if (!box || !root) return;
    const sync = () => {
      const r = box.getBoundingClientRect();
      root.style.setProperty("--main-l", `${r.left}px`);
      root.style.setProperty("--main-r", `${Math.max(0, document.documentElement.clientWidth - r.right)}px`);
    };
    const observer = new ResizeObserver(sync);
    observer.observe(box);
    sync();
    return () => observer.disconnect();
  }, []);
const leftPanel = usePanelRef();
  const rightPanel = usePanelRef();
  const panelGroup = useRef<HTMLDivElement>(null);
  const placed = useRef(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [plugins, setPlugins] = useState<PluginView[]>([]);
  /** The first plugins list arrived, so a view tab whose plugin isn't in it is gone for good. */
  const [pluginsLoaded, setPluginsLoaded] = useState(false);
  const [droppedPlugin, setDroppedPlugin] = useState<string | null>(null);
  // A plugin the CLI installed or tried to turn on, waiting for approval in Settings, Plugins.
  const [reviewPlugin, setReviewPlugin] = useState<string | null>(null);
  /** The plugin sidebar on the right, as `pluginId:sidebarId`, and every one opened so far (kept running). */
  const [rightSidebar, setRightSidebar] = useState<string | null>(null);
  const [openedSidebars, setOpenedSidebars] = useState<string[]>([]);
  // The sidebar page the right edge is peeking, so it stays loaded while shown.
  const [peekedSidebar, setPeekedSidebar] = useState<string | null>(null);
  /** What plugins show under each project in the sidebar, and which of those you folded shut. */
  const [projectSections, setProjectSections] = useState<ProjectSections>({});
  const [collapsedSections, setCollapsedSections] = useState<string[]>([]);
  const [projectSort, setProjectSort] = useState<ProjectSort>("manual");
  const [olderOpen, setOlderOpen] = useState(false);
  /** When each project was last worked on, for the sidebar's sort and its "Older" group. Written at most once a minute per project. */
  const [lastActive, setLastActive] = useState<Record<string, number>>({});
  /** The Inbox, newest first; saved with the workspace so it outlasts a restart or an update. */
  const [notifications, notify] = useReducer(notificationsReducer, []);
  const [sidebarLabels, setSidebarLabels] = useState<Record<string, SidebarLabel>>({});
  /** The plugin page shown in a modal, as `pluginId:viewId`. */
  const [modal, setModal] = useState<string | null>(null);
  /** Bumped when a plugin stops, so its sidebar pages mount fresh next time. */
  const [epochs, setEpochs] = useState<Record<string, number>>({});

  const activePlugins = useMemo(() => plugins.filter((p) => p.enabled && p.approved), [plugins]);
  /** A page of a plugin that's on, by `pluginId:viewId`, if its manifest lets it open in `place`. */
  const pageRef = useCallback(
    (key: string, place: "sidebar" | "modal") => {
      const [pluginId, viewId] = key.split(":");
      const plugin = activePlugins.find((p) => p.id === pluginId);
      if (!plugin || !pagesOf(plugin).some((v) => v.id === viewId && v.opens.includes(place))) return null;
      return { plugin, viewId, id: key, key: `${key}:${plugin.version}:${epochs[pluginId] ?? 0}` };
    },
    [activePlugins, epochs],
  );
  const sidebarRef = useCallback(
    (key: string): SidebarRef | null => {
      const ref = pageRef(key, "sidebar");
      return ref && { plugin: ref.plugin, sidebarId: ref.viewId, id: ref.id, key: ref.key };
    },
    [pageRef],
  );
  const modalRef: ModalRef | null = modal ? pageRef(modal, "modal") : null;
  /** A sidebar's popover, keyed like its page so a restarted plugin loads it again. */
  const popoverOf = (plugin: PluginView, sidebarId: string) => {
    const popover = plugin.contributes.sidebars.find((s) => s.id === sidebarId)?.popover;
    return popover && { width: popover.width, height: popover.height, key: `${plugin.id}:${sidebarId}:popover:${plugin.version}:${epochs[plugin.id] ?? 0}` };
  };

  // Dropping a .wings-plugin file anywhere on the window opens the manager and installs it.
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const file = event.payload.paths.find((p) => p.endsWith(".wings-plugin"));
      if (!file) return;
      openPlugins();
      setDroppedPlugin(file);
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);
  const [ready, setReady] = useState(false);
  const [fontSize, setFontSize] = useState(DEFAULT_FONT_SIZE);
  const [terminalKeys, setTerminalKeysState] = useState<TerminalKeys>(DEFAULT_TERMINAL_KEYS);
  const [terminalLook, setTerminalLookState] = useState<TerminalLook>(DEFAULT_TERMINAL_LOOK);
  const [globalHotkey, setGlobalHotkey] = useState<GlobalHotkey>(DEFAULT_GLOBAL_HOTKEY);
  const [sleep, setSleep] = useState<SleepSettings>(DEFAULT_SLEEP);
  const [notifySound, setNotifySound] = useState<SoundSettings>(DEFAULT_SOUND);
  const [agentsRunCommands, setAgentsRunCommands] = useState(false);
  const agentsRunRef = useRef(agentsRunCommands);
  agentsRunRef.current = agentsRunCommands;
  const [sleepTick, setSleepTick] = useState(0);
  const [newTabPage, setNewTabPage] = useState<string>("terminal");
  const [newProjectOpens, setNewProjectOpens] = useState<NewProjectOpens>("terminal");
  const newProjectOpensRef = useRef(newProjectOpens);
  newProjectOpensRef.current = newProjectOpens;
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("text");
  const [settingsPlugin, setSettingsPlugin] = useState<string | null>(null);
  /** Opens Settings on Plugins, on that plugin when there is one. */
  const openPlugins = useCallback((id?: string) => {
    setSettingsSection("plugins");
    if (id) setSettingsPlugin(id);
    setSettingsOpen(true);
  }, []);
  const pluginUpdates = usePluginUpdates(plugins, openPlugins);
  /** Plugin badges by Rust pane id, then plugin id. */
  const [badges, setBadges] = useState<Record<string, Record<string, Badge>>>({});
  /** Plugin pane statuses (the strip under a terminal) by Rust pane id, then plugin id. */
  const [statuses, setStatuses] = useState<Record<string, Record<string, PaneStatus>>>({});
  /** Plugin rails (the card over a terminal) by Rust pane id, then plugin id. */
  const [rails, setRails] = useState<Record<string, Record<string, PaneRail>>>({});
  const [diff, setDiff] = useState<DiffView | null>(null);
  const pluginHost = useRef<PluginHost | null>(null);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const panesRef = useRef(panes);
  panesRef.current = panes;
  const spacesRef = useRef(spaces);
  spacesRef.current = spaces;
  const gitRef = useRef(git);
  gitRef.current = git;
  const pluginsRef = useRef(plugins);
  pluginsRef.current = plugins;
  /** A project as plugins see it: its branch as it is now. */
  const pluginProject = useCallback((space: Space) => ({ id: space.id, name: space.name, path: space.path, branch: gitRef.current[space.id]?.branch ?? space.branch }), []);
  const paneInfoRef = useRef(paneInfo);
  paneInfoRef.current = paneInfo;

  const activeSpace = spaces.find((s) => s.id === activeSpaceId) ?? null;
  const spaceTabs = tabs.filter((t) => t.spaceId === activeSpaceId);
  const activeTabId = activeSpaceId ? (activeTabBySpace[activeSpaceId] ?? spaceTabs[0]?.id ?? null) : null;
  const shownTab = tabs.find((t) => t.id === activeTabId) ?? null;
  /** The terminal tab on screen; null while a plugin view's tab shows. */
  const activeTab = shownTab && isPaneTab(shownTab) ? shownTab : null;
  const shownView = shownTab && isViewTab(shownTab) ? `${shownTab.view.pluginId}:${shownTab.view.viewId}` : null;
  const pluginButtons: PluginButton[] = activePlugins.flatMap((p) => [
    // A sidebar or view button opens its page in the first place its manifest lists.
    ...pagesOf(p).filter((s) => s.button !== false).map((s) => ({
      key: `${p.id}:${s.id}`,
      pluginId: p.id,
      kind: s.opens[0],
      id: s.id,
      title: s.title,
      icon: s.icon,
      iconSvg: s.iconSvg,
      label: sidebarLabels[`${p.id}:${s.id}`]?.text,
      rows: sidebarLabels[`${p.id}:${s.id}`]?.rows,
      tone: sidebarLabels[`${p.id}:${s.id}`]?.tone,
      active: (s.opens[0] === "sidebar" ? rightSidebar : s.opens[0] === "tab" ? shownView : modal) === `${p.id}:${s.id}`,
      popover: popoverOf(p, s.id),
    })),
    ...p.contributes.panels.map((panel) => ({ key: `${p.id}:panel:${panel.id}`, pluginId: p.id, kind: "panel" as const, id: panel.id, title: panel.title, icon: panel.icon, iconSvg: p.iconSvg })),
  ]);
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;
  // The tab shown before this one keeps its terminals' WebGL renderers: rebuilding one takes about 10 ms a pane and
  // releasing one about 4, so switching back and forth took 8 ms instead of 40. Older tabs release theirs, since each
  // kept renderer holds GPU memory and WebKit allows 16 WebGL contexts per page.
  const recentTabs = useRef<string[]>([]);
  if (activeTabId && recentTabs.current[0] !== activeTabId) {
    recentTabs.current = [activeTabId, ...recentTabs.current.filter((id) => id !== activeTabId)].slice(0, 2);
  }
  // Plugin pages not on screen for a while unload and load again when shown (a page asks its plugin for its state).
  const unloadedSidebars = useIdleUnload(openedSidebars, rightSidebar ?? peekedSidebar, SIDEBAR_UNLOAD_MS);
  const unloadedViewTabs = useIdleUnload(tabs.filter(isViewTab).map((t) => t.id), activeTabId, TAB_PAGE_UNLOAD_MS);
  const activeSpaceIdRef = useRef(activeSpaceId);
  activeSpaceIdRef.current = activeSpaceId;
  const focusedPaneId = activeTab ? (panes[activeTab.focusedPane]?.paneId ?? null) : null;
  // Rows for sessions with no live agent: restored ones whose tab hasn't been opened yet, sleeping ones, and ones
  // being resumed (or that couldn't be), so the Agents list never drops a session while it is coming back.
  const [sessionTitles, setSessionTitles] = useState<Record<string, string | null>>({});
  const agentsByPane = useMemo(() => new Map(agents.map((a) => [a.paneId, a])), [agents]);
  const pending = useMemo(
    () =>
      Object.entries(panes).flatMap(([key, meta]) => {
        const phase = phaseOf(meta, meta.paneId ? agentsByPane.get(meta.paneId) : undefined);
        return phase ? [{ key, spaceId: meta.spaceId, kind: meta.kind, phase, title: meta.name ?? (meta.sessionId ? sessionTitles[meta.sessionId] : null) ?? null }] : [];
      }),
    [panes, agentsByPane, sessionTitles],
  );
  const pendingSpaces = [...new Set(pending.filter((r) => r.kind === "claude").map((r) => r.spaceId))].join(" ");
  useEffect(() => {
    for (const spaceId of pendingSpaces.split(" ").filter(Boolean)) {
      void api.sessionsList(spaceId).then((list) =>
        setSessionTitles((t) => ({ ...t, ...Object.fromEntries(list.map((s) => [s.id, s.title ?? s.firstPrompt])) })),
      );
    }
  }, [pendingSpaces]);
  const liveSessionIds = useMemo(() => new Set(agents.flatMap((a) => (a.sessionId ? [a.sessionId] : []))), [agents]);

  /** The names you gave panes, kept in their tabs' layouts. */
  const names = useMemo(() => Object.assign({}, ...tabs.map((t) => (isPaneTab(t) ? paneNames(t.layout) : {}))) as Record<string, string>, [tabs]);
  const labels = useMemo(() => {
    const out: Record<string, PaneLabelInfo> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const agent = meta.paneId ? agentsByPane.get(meta.paneId) : undefined;
      const info = meta.paneId ? paneInfo[meta.paneId] : undefined;
      const label = paneLabel(agent, info, spaces.find((s) => s.id === meta.spaceId));
      out[key] = names[key] ? { ...label, title: names[key] } : label;
    }
    return out;
  }, [panes, agentsByPane, paneInfo, spaces, names]);

  const updateTab = useCallback((tabId: string, patch: (t: PaneTab) => Partial<PaneTab>) => {
    setTabs((ts) => ts.map((t) => (t.id === tabId && isPaneTab(t) ? { ...t, ...patch(t) } : t)));
  }, []);

  const touchSpace = useCallback((spaceId: string) => setLastActive((m) => touched(m, [spaceId], Date.now())), []);
  // Selecting a project, however it happens, counts as working on it.
  useEffect(() => {
    if (activeSpaceId) touchSpace(activeSpaceId);
  }, [activeSpaceId, touchSpace]);
  // A project that has no time yet (first run with this, or just added) counts as worked on now.
  useEffect(() => {
    if (ready) setLastActive((m) => withMissing(m, spaces.map((s) => s.id), Date.now()));
  }, [ready, spaces]);

  const lastAgentStates = useRef(new Map<string, AgentState>());
  const focusPane = useCallback((key: string) => {
    const tab = tabOfPane(tabsRef.current, key);
    if (tab && tab.focusedPane !== key) updateTab(tab.id, () => ({ focusedPane: key }));
    if (tab) touchSpace(tab.spaceId);
  }, [updateTab, touchSpace]);

  /** Creates the terminal for a new pane; it starts its shell once its card mounts. */
  const createPane = useCallback(
    (
      spaceId: string,
      initialInput: string | null,
      sessionId: string | null = null,
      cwd: string | null = null,
      args: string[] = [],
      attachTo: string | null = null,
      kind: SessionKind = "claude",
      savedCwd: string | null = null,
      historyId: string | null = null,
    ) => {
      const key = crypto.randomUUID();
      historyId ??= key;
      // A pane that starts with a resume line: its agent isn't detected for a moment after the pane starts, and the
      // Agents row holds its place as "starting" until it is.
      const resuming = !!sessionId && !!initialInput && !attachTo;
      terminals.set(
        key,
        new TerminalSession(
          spaceId,
          {
            onFocus: () => focusPane(key),
            onStarted: (paneId) =>
              setPanes((p) => ({ ...p, [key]: { ...p[key], paneId, ...(resuming && p[key].resumeAt === null ? { ...noResume, resumeAt: Date.now() } : {}) } })),
          },
          cwd,
          attachTo,
          savedCwd,
          historyId,
        ),
      );
      // A pane still running from before a UI reload is known by its Rust id now, before its tab is first shown.
      setPanes((p) => ({ ...p, [key]: { spaceId, paneId: attachTo, sessionId, kind, name: null, live: false, args, ...noResume, savedCwd, historyId } }));
      setInitialInputs((m) => ({ ...m, [key]: initialInput }));
      return key;
    },
    [focusPane],
  );

  const openTab = useCallback(
    (spaceId: string, initialInput: string | null = null, sessionId: string | null = null, cwd: string | null = null, kind: SessionKind = "claude") => {
      const key = createPane(spaceId, initialInput, sessionId, cwd, [], null, kind);
      const id = crypto.randomUUID();
      setTabs((ts) => [...ts, { id, spaceId, layout: pane(key), focusedPane: key, zoomedPane: null }]);
      setActiveTabBySpace((m) => ({ ...m, [spaceId]: id }));
      setActiveSpaceId(spaceId);
      return key;
    },
    [createPane],
  );

  // Claude's `open_pane` tool opens its tab here like any other, so the workspace and the pane list stay one thing.
  useEffect(() => {
    const off = api.onMcpOpenPane(async (call) => {
      try {
        const ran = call.run && call.input !== null && agentsRunRef.current;
        const key = openTab(call.spaceId, ran ? `${call.input}\r` : call.input, null, call.cwd);
        const paneId = await terminals.get(key)?.whenStarted;
        if (!paneId) throw new Error("The pane didn't start");
        void api.mcpToolResult(call.callId, JSON.stringify({ paneId, ran }), false);
      } catch (e) {
        void api.mcpToolResult(call.callId, e instanceof Error ? e.message : String(e), true);
      }
    });
    return () => void off.then((unlisten) => unlisten());
  }, [openTab]);

  const selectSpace = useCallback(
    (spaceId: string) => {
      setActiveSpaceId(spaceId);
      if (!tabsRef.current.some((t) => t.spaceId === spaceId)) openTab(spaceId);
    },
    [openTab],
  );

  const disposePane = (key: string) => {
    terminals.get(key)?.dispose();
    terminals.delete(key);
    setPanes(({ [key]: _gone, ...rest }) => rest);
  };

  /** Closes tabs; a project showing one of them shows its last other tab. */
  const closeTabs = useCallback((tabIds: string[]) => {
    const gone = tabsRef.current.filter((t) => tabIds.includes(t.id));
    if (!gone.length) return;
    for (const tab of gone) if (isPaneTab(tab)) paneIds(tab.layout).forEach(disposePane);
    const rest = tabsRef.current.filter((t) => !tabIds.includes(t.id));
    setTabs(rest);
    setActiveTabBySpace((m) => {
      const next = { ...m };
      for (const tab of gone) {
        if (next[tab.spaceId] !== tab.id) continue;
        const siblings = rest.filter((t) => t.spaceId === tab.spaceId);
        if (siblings.length) next[tab.spaceId] = siblings[siblings.length - 1].id;
        else delete next[tab.spaceId];
      }
      return next;
    });
  }, []);
  const closeTab = useCallback((tabId: string) => closeTabs([tabId]), [closeTabs]);

  /** Shows a plugin view's tab in a project, opening it if it isn't there yet. */
  const showViewTab = useCallback((spaceId: string, pluginId: string, viewId: string, options?: { multiple: boolean; args?: unknown; newTab?: boolean }) => {
    const opened = openViewTab(tabsRef.current, spaceId, { pluginId, viewId }, crypto.randomUUID(), options);
    if (opened.tabs !== tabsRef.current) setTabs(opened.tabs);
    setActiveTabBySpace((m) => ({ ...m, [spaceId]: opened.id }));
    setActiveSpaceId(spaceId);
  }, []);

  /** Shows a project's Project page, opening its tab if it isn't there yet. */
  const showProjectPage = useCallback((spaceId: string) => {
    const opened = openProjectTab(tabsRef.current, spaceId, crypto.randomUUID());
    if (opened.tabs !== tabsRef.current) setTabs(opened.tabs);
    setActiveTabBySpace((m) => ({ ...m, [spaceId]: opened.id }));
    setActiveSpaceId(spaceId);
  }, []);

  // Back and forward. Every change of project or tab, whatever caused it, arrives here as the place on screen, so
  // nothing that navigates has to know about the history. Going back sets the place, which then visits itself: no change.
  const navRef = useRef<NavState>(EMPTY_NAV);
  const [nav, setNav] = useState<NavState>(EMPTY_NAV);
  const placeExists = useCallback((p: Place) => tabsRef.current.some((t) => t.id === p.tab && t.spaceId === p.space), []);
  useEffect(() => {
    const next = visit(navRef.current, activeSpaceId && activeTabId ? { space: activeSpaceId, tab: activeTabId } : null);
    if (next === navRef.current) return;
    navRef.current = next;
    setNav(next);
  }, [activeSpaceId, activeTabId]);
  const navigate = useCallback(
    (go: typeof goBack) => {
      const next = go(navRef.current, placeExists);
      if (next === navRef.current) return;
      navRef.current = next;
      setNav(next);
      if (!next.current) return;
      setActiveTabBySpace((m) => ({ ...m, [next.current!.space]: next.current!.tab }));
      setActiveSpaceId(next.current.space);
    },
    [placeExists],
  );
  const canBack = canGoBack(nav, placeExists);
  const canForward = canGoForward(nav, placeExists);

  // A view tab goes once its plugin is off or removed, or its view stops opening as a tab, as on restore.
  useEffect(() => {
    if (pluginsLoaded) closeTabs(staleViewTabs(tabs, plugins));
  }, [pluginsLoaded, plugins, tabs, closeTabs]);

  const closePane = useCallback(
    (key: string) => {
      const tab = tabOfPane(tabsRef.current, key);
      if (!tab) return;
      const layout = remove(tab.layout, key);
      if (!layout) return closeTab(tab.id);
      disposePane(key);
      updateTab(tab.id, (t) => ({
        layout,
        focusedPane: t.focusedPane === key ? paneIds(layout)[0] : t.focusedPane,
        zoomedPane: t.zoomedPane === key ? null : t.zoomedPane,
      }));
    },
    [closeTab, updateTab],
  );

  const splitPane = useCallback(
    (key: string, dir: "row" | "column", initialInput: string | null = null, cwd: string | null = null) => {
      const tab = tabOfPane(tabsRef.current, key);
      if (!tab) return;
      const next = createPane(tab.spaceId, initialInput, null, cwd);
      updateTab(tab.id, (t) => ({ layout: split(t.layout, key, dir, next), focusedPane: next, zoomedPane: null }));
      return next;
    },
    [createPane, updateTab],
  );

  /** Names a tab; an empty name goes back to the automatic one. */
  const renameTab = useCallback((tabId: string, name: string) => {
    setTabs((ts) =>
      ts.map((t) => {
        if (t.id !== tabId || !isPaneTab(t)) return t;
        const { name: _old, ...rest } = t;
        return name.trim() ? { ...rest, name: name.trim() } : rest;
      }),
    );
    // Back to the terminal you were typing in.
    requestAnimationFrame(() => {
      const tab = tabsRef.current.find((t) => t.id === tabId);
      if (tab && isPaneTab(tab)) terminals.get(tab.focusedPane)?.focus();
    });
  }, []);

  /** Moves a tab to a place among its project's tabs. */
  const moveTabTo = useCallback((tabId: string, index: number) => setTabs((ts) => moveTab(ts, tabId, index)), []);

  /** Moves a pane into another tab of its project, or into a new one; the terminal keeps running, only its card moves. */
  const movePane = useCallback((key: string, target: string | "new") => {
    const from = tabOfPane(tabsRef.current, key);
    const leaf = from && findPane(from.layout, key);
    if (!from || !leaf || target === from.id) return;
    const to = target === "new" ? null : tabsRef.current.find((t): t is PaneTab => t.id === target && isPaneTab(t) && t.spaceId === from.spaceId);
    if (target !== "new" && !to) return;
    const rest = remove(from.layout, key);
    const newId = crypto.randomUUID();
    setTabs((ts) => {
      const moved = ts.flatMap((t): Tab[] => {
        if (t.id === from.id) return rest ? [{ ...(t as PaneTab), layout: rest, focusedPane: (t as PaneTab).focusedPane === key ? paneIds(rest)[0] : (t as PaneTab).focusedPane, zoomedPane: (t as PaneTab).zoomedPane === key ? null : (t as PaneTab).zoomedPane }] : [];
        if (to && t.id === to.id) {
          const dest = t as PaneTab;
          return [{ ...dest, layout: insertPane(dest.layout, dest.focusedPane, "row", leaf), focusedPane: key, zoomedPane: null }];
        }
        return [t];
      });
      return to ? moved : [...moved, { id: newId, spaceId: from.spaceId, layout: leaf, focusedPane: key, zoomedPane: null }];
    });
    setActiveTabBySpace((m) => ({ ...m, [from.spaceId]: to?.id ?? newId }));
  }, []);

  const actions: PaneActions = useMemo(
    () => ({
      focus: focusPane,
      split: splitPane,
      close: closePane,
      toggleZoom: (key) => {
        const tab = tabOfPane(tabsRef.current, key);
        if (tab) updateTab(tab.id, (t) => ({ zoomedPane: t.zoomedPane === key ? null : key, focusedPane: key }));
      },
      rename: (key, name) => {
        const tab = tabOfPane(tabsRef.current, key);
        if (tab) updateTab(tab.id, (t) => ({ layout: renamePane(t.layout, key, name) }));
      },
      moveTo: movePane,
      resize: (splitId, ratio) => {
        const tab = tabsRef.current.find((t) => t.id === activeTabId);
        if (tab) updateTab(tab.id, (t) => ({ layout: setRatio(t.layout, splitId, ratio) }));
      },
    }),
    [focusPane, splitPane, closePane, updateTab, movePane, activeTabId],
  );

  /**
   * Rebuilds saved tabs and splits. After a webview reload, panes in `running` show their still-running shell again;
   * the others get a fresh shell, and Claude panes resume their session.
   */
  const restore = useCallback(
    (list: Space[], saved: SavedWorkspace | null, running: Map<string, string>) => {
      if (!saved) return false;
      // Before any terminal exists: a later font change resizes every pane right after its first draw,
      // which leaves Claude Code's screen garbled until it repaints.
      if (saved.fontSize) {
        setTerminalFontSize(saved.fontSize);
        setFontSize(saved.fontSize);
      }
      if (saved.terminalKeys) setTerminalKeysState({ ...DEFAULT_TERMINAL_KEYS, ...saved.terminalKeys });
      if (saved.globalHotkey) setGlobalHotkey({ ...DEFAULT_GLOBAL_HOTKEY, ...saved.globalHotkey });
      if (saved.terminalLook) setTerminalLookState({ ...DEFAULT_TERMINAL_LOOK, ...saved.terminalLook });
      if (saved.pinnedButtons) setPinnedButtons(saved.pinnedButtons);
      if (saved.newTabPage) setNewTabPage(saved.newTabPage);
      setNewProjectOpens(parseNewProjectOpens(saved.newProjectOpens));
      setCollapsedSections(restoreCollapsed(saved.collapsedSections, list.map((s) => s.id)));
      setProjectSort(parseProjectSort(saved.projectSort));
      setOlderOpen(saved.olderOpen === true);
      setLastActive(restoreLastActive(saved.lastActive, list.map((s) => s.id)));
      notify({ type: "restore", list: restoreNotifications(saved.notifications, new Set(running.values())) });
      if (saved.sleep) setSleep({ ...DEFAULT_SLEEP, ...saved.sleep });
      setNotifySound(cleanSound(saved.notifySound));
      setAgentsRunCommands(saved.agentsRunCommands === true);
      const known = new Set(list.map((s) => s.id));
      const asleepKeys: string[] = [];
      const restored = saved.tabs
        .filter((t) => known.has(t.spaceId))
        .map((t) => {
          const keys: Record<string, string> = {};
          const layout = mapPanes(t.layout, (old) => {
            const session = saved.sessions[old];
            const kind = saved.sessionKinds?.[old] ?? "claude";
            const resume = session && isSessionIdOf(kind, session) ? session : null;
            const args = resume ? (saved.sessionArgs?.[old] ?? []) : [];
            // The resume command runs only if the pane can't attach: the agent still runs in an attached one.
            keys[old] = createPane(t.spaceId, resume && resumeLine(kind, resume, args), resume, null, args, running.get(old) ?? null, kind, saved.cwds?.[old] ?? null, saved.histories?.[old] ?? null);
            // Still asleep until opened, as Wings left it.
            if (resume && saved.asleep?.includes(old) && !running.has(old)) asleepKeys.push(keys[old]);
            return keys[old];
          });
          const focusedPane = keys[t.focusedPane] ?? paneIds(layout)[0];
          return { ...t, layout, focusedPane, zoomedPane: t.zoomedPane ? (keys[t.zoomedPane] ?? null) : null };
        });
      if (asleepKeys.length) setPanes((p) => ({ ...p, ...Object.fromEntries(asleepKeys.map((k) => [k, { ...p[k], asleep: true }])) }));
      const all = restoreTabs<PaneTab>(restored, saved.viewTabs?.filter((t) => known.has(t.spaceId)), saved.projectTabs?.filter((t) => known.has(t.spaceId)));
      if (!all.length) return false;
      const tabIds = new Set(all.map((t) => t.id));
      setTabs(all);
      setActiveTabBySpace(Object.fromEntries(Object.entries(saved.activeTabBySpace).filter(([, id]) => tabIds.has(id))));
      setSidebarOpen(saved.sidebarOpen);
      if (saved.widths) setWidths({ left: fit("left", saved.widths.left), right: fit("right", saved.widths.right) });
      setActiveSpaceId(saved.activeSpaceId && known.has(saved.activeSpaceId) ? saved.activeSpaceId : all[0].spaceId);
      return true;
    },
    [createPane],
  );

  // Initial load and live events from the Rust core.
  useEffect(() => {
    void (async () => {
      const [list, json, live] = await Promise.all([api.spacesList(), api.workspaceLoad(), api.panesList()]);
      const saved = parseSaved(json);
      const running = stillRunning(saved, live, list);
      // Before any pane starts, so only panes from before a reload that nothing shows again are killed.
      await api.panesReset([...running.values()]);
      void api.historiesKeep(Object.values(saved?.histories ?? {}));
      setSpaces(sortByOrder(list, saved?.spaceOrder));
      if (!restore(list, saved, running) && list[0]) selectSpace(list[0].id);
      setReady(true);
    })();
    void api.agentsList().then(setAgents);
    void api.paneInfo().then(setPaneInfo);
    void api.gitStatus().then(setGit);
    const unlisten = [
      api.onAgents((list) => {
        setAgents(list);
        // An agent changing state (not each output chunk) counts as work in its project.
        const changed = list.filter((a) => lastAgentStates.current.get(a.paneId) !== a.state);
        lastAgentStates.current = new Map(list.map((a) => [a.paneId, a.state]));
        for (const id of new Set(changed.map((a) => a.spaceId))) touchSpace(id);
      }),
      api.onPaneInfo(setPaneInfo),
      api.onGitStatus(setGit),
      api.onPaneExited((paneId) => {
        const key = Object.keys(panesRef.current).find((k) => panesRef.current[k].paneId === paneId);
        if (key) closePane(key);
      }),
    ];
    return () => unlisten.forEach((p) => void p.then((off) => off()));
  }, [selectSpace, closePane, restore, touchSpace]);

  // Remember which agent session each pane runs, so a restart or a sleep can resume it. An agent seen in a pane
  // ends its resume: the row has handed over to the live one.
  useEffect(() => {
    setPanes((prev) => {
      let next: Record<string, PaneMeta> | null = null;
      for (const [key, meta] of Object.entries(prev)) {
        const agent = meta.paneId ? agentsByPane.get(meta.paneId) : undefined;
        let patch: Partial<PaneMeta> | null = null;
        if (agent && (settled(meta, agent) || (agent.sessionId && (agent.sessionId !== meta.sessionId || agent.kind !== meta.kind || !meta.live)))) {
          patch = { ...(agent.sessionId ? { sessionId: agent.sessionId, kind: agent.kind, args: agent.args } : {}), name: agent.name ?? meta.name, live: true, ...noResume };
        } else if (!agent && meta.live && !meta.asleep && meta.resumeAt === null) patch = { sessionId: null, live: false, args: [] };
        if (patch) {
          next ??= { ...prev };
          next[key] = { ...meta, ...patch };
        }
      }
      return next ?? prev;
    });
  }, [agentsByPane]);

  /** The workspace while its save waits out the debounce. */
  const unsaved = useRef<string | null>(null);
  /** Reloads only the UI. Saves first, or a pane opened just before would be closed instead of shown again. */
  const reloadUi = useCallback(() => {
    void Promise.resolve(unsaved.current && api.workspaceSave(unsaved.current)).finally(() => location.reload());
  }, []);

  // A string, so the save below reruns when a folder changes and not on every other pane-info update.
  const cwdsJson = useMemo(() => JSON.stringify(paneCwds(panes, paneInfo)), [panes, paneInfo]);
  useEffect(() => {
    if (!ready) return;
    const sessions =Object.fromEntries(Object.entries(panes).map(([key, meta]) => [key, meta.sessionId]));
    const sessionKinds = Object.fromEntries(Object.entries(panes).flatMap(([key, meta]) => (meta.sessionId ? [[key, meta.kind]] : [])));
    const asleep = Object.entries(panes).flatMap(([key, meta]) => (meta.asleep && meta.sessionId ? [key] : []));
    const sessionArgs = Object.fromEntries(Object.entries(panes).flatMap(([key, meta]) => (meta.sessionId && meta.args.length ? [[key, meta.args]] : [])));
    const livePanes = Object.fromEntries(Object.entries(panes).flatMap(([key, meta]) => (meta.paneId ? [[key, meta.paneId]] : [])));
    const { tabs: paneTabs, viewTabs, projectTabs } = saveTabs<PaneTab>(tabs);
    const saved: SavedWorkspace = {
      version: 1,
      activeSpaceId,
      activeTabBySpace,
      spaceOrder: spaces.map((s) => s.id),
      sidebarOpen,
      widths,
      pinnedButtons,
      fontSize,
      terminalKeys,
      globalHotkey,
      terminalLook,
      tabs: paneTabs,
      viewTabs,
      projectTabs,
      sessions,
      sessionKinds,
      asleep,
      sleep,
      notifySound,
      agentsRunCommands,
      newTabPage,
      newProjectOpens,
      collapsedSections,
      projectSort,
      olderOpen,
      lastActive,
      notifications,
      sessionArgs,
      cwds: JSON.parse(cwdsJson),
      paneIds: livePanes,
      histories: Object.fromEntries(Object.entries(panes).map(([key, meta]) => [key, meta.historyId])),
    };
    const json = JSON.stringify(saved);
    unsaved.current = json;
    const timer = setTimeout(() => {
      unsaved.current = null;
      void api.workspaceSave(json);
    }, 400);
    return () => clearTimeout(timer);
  }, [ready, tabs, panes, spaces, activeSpaceId, activeTabBySpace, sidebarOpen, widths, pinnedButtons, fontSize, terminalKeys, globalHotkey, terminalLook, sleep, notifySound, agentsRunCommands, newTabPage, newProjectOpens, collapsedSections, projectSort, olderOpen, lastActive, notifications, cwdsJson]);

  const rightRef = rightSidebar ? sidebarRef(rightSidebar) : null;
  // Sidebars slide open and shut; a drag on a handle resizes them straight away. The first placement, with the
  // saved widths, doesn't slide.
  useEffect(() => {
    if (!ready) return;
    const group = panelGroup.current;
    const first = !placed.current;
    placed.current = true;
    if (group && !first) group.dataset.sliding = "";
    place(leftPanel.current, sidebarOpen, widths.left, first);
    place(rightPanel.current, rightRef !== null, widths.right, first);
    const timer = setTimeout(() => group && delete group.dataset.sliding, 340);
    return () => clearTimeout(timer);
  }, [ready, sidebarOpen, rightRef !== null]);

  // A pane only counts as seen while the window has focus, so "done" survives you switching apps.
  useEffect(() => {
    const sync = () => void api.paneFocus(document.hasFocus() ? focusedPaneId : null);
    sync();
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    return () => {
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
    };
  }, [focusedPaneId]);

  // Every shortcut, menu item and palette entry runs through here. A menu item with an accelerator can reach the
  // window as a key press as well as a menu event, so the same command twice within a few ms runs once.
  const commandsRef = useRef<ReturnType<typeof createCommands>>([]);
  const lastRun = useRef(new Map<string, number>());
  const runCommand = useCallback((id: string) => {
    const now = performance.now();
    if (now - (lastRun.current.get(id) ?? -Infinity) < 25) return;
    lastRun.current.set(id, now);
    if (id.startsWith("tab:")) {
      const spaceId = activeSpaceIdRef.current;
      const tab = tabsRef.current.filter((t) => t.spaceId === spaceId)[Number(id.slice(4))];
      if (spaceId && tab) setActiveTabBySpace((m) => ({ ...m, [spaceId]: tab.id }));
      return;
    }
    const command = commandsRef.current.find((c) => c.id === id);
    if (command?.enabled) command.run();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = shortcutFor(e);
      if (!id) return;
      e.preventDefault();
      runCommand(id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [runCommand]);

  // The mouse's back and forward buttons are buttons 3 and 4 (WebKit maps NSEvent buttonNumber 3 and 4 to them).
  // Capture phase on the window, so a terminal can't take them, and the press is swallowed too: a program with
  // mouse reporting on would get a stray button otherwise.
  useEffect(() => {
    const side = (e: MouseEvent) => {
      if (e.button !== 3 && e.button !== 4) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.type === "mouseup") runCommand(e.button === 3 ? "go-back" : "go-forward");
    };
    const types = ["mousedown", "mouseup", "auxclick"] as const;
    for (const type of types) window.addEventListener(type, side, true);
    return () => {
      for (const type of types) window.removeEventListener(type, side, true);
    };
  }, [runCommand]);

  // The menu owns ⌘W (so it never closes the window), and its Tab, Pane and View items are the commands of
  // `commands.json`; it forwards them here.
  useEffect(() => {
    const unlisten = api.onMenu((id) => {
      runCommand(id.startsWith("cmd:") ? id.slice(4) : id);
    });
    return () => void unlisten.then((off) => off());
  }, [runCommand]);

  // The global shortcut is held only while it is on. Changing it releases the old one first.
  useEffect(() => {
    if (!ready) return;
    api.hotkeySet(globalHotkey.enabled ? globalHotkey.accelerator : null).catch((e) => {
      toast.error("Couldn't use that global shortcut", { id: "global-hotkey", description: String(e) });
    });
  }, [ready, globalHotkey]);

  // Pressed from another app, Wings is already in front: go to the agent that needs you, or stay where you were.
  useEffect(() => {
    const unlisten = api.onGlobalHotkey(() => {
      const next = nextNeedingYou(agentsRef.current, null);
      if (next) selectAgentRef.current(next);
    });
    return () => void unlisten.then((off) => off());
  }, []);

  useEffect(() => setTerminalFontSize(fontSize), [fontSize]);
  useEffect(() => setTerminalKeys(terminalKeys), [terminalKeys]);
  useEffect(() => setTerminalLook(terminalLook), [terminalLook]);
  useEffect(() => setSoundSettings(notifySound), [notifySound]);

  // Wings sends a system notification only while it's in the background (`notify_transitions` in lib.rs).
  // In front, it's a toast instead, unless you're already looking at that pane.
  const agentStates = useRef(new Map<string, AgentState>());
  const selectAgentRef = useRef(selectAgent);
  selectAgentRef.current = selectAgent;
  const agentsRef = useRef(agents);
  agentsRef.current = agents;
  useEffect(() => {
    const before = agentStates.current;
    agentStates.current = new Map(agents.map((a) => [a.paneId, a.state]));
    for (const paneId of before.keys()) if (!agentStates.current.has(paneId)) toast.dismiss(`agent-${paneId}`);
    for (const agent of agents) {
      const was = before.get(agent.paneId);
      if (was === undefined || was === agent.state) continue;
      const id = `agent-${agent.paneId}`;
      if (was === "blocked") toast.dismiss(id);
      if (agent.state !== "blocked" && agent.state !== "done") continue;
      const name = agent.name ?? agentLabel(agent.kind);
      const project = spaces.find((s) => s.id === agent.spaceId)?.name ?? "";
      const title = agent.state === "blocked" ? `${name} needs you` : `${name} finished`;
      const eventId = `${agent.paneId}:${Date.now()}`;
      // Looking at the pane with Wings in front counts as seen.
      const seen = document.hasFocus() && agent.paneId === focusedPaneId;
      notify({
        type: "add",
        event: { id: eventId, at: Date.now(), kind: agent.state, title, project, detail: agent.state === "blocked" ? (agent.waitingFor ?? "waiting for input") : "", paneId: agent.paneId, read: seen },
      });
      notificationSound(seen);
      // Looking at it already, so no toast. Otherwise it shows, in front or in the background.
      if (seen) continue;
      if (!document.hasFocus()) continue;
      toast(title, {
        id,
        onDismiss: () => notify({ type: "read", id: eventId }),
        icon: <StatusDot state={agent.state} />,
        description: agent.state === "blocked" ? `${project}: ${agent.waitingFor ?? "waiting for input"}` : project,
        // Waiting on you stays up until you answer.
        duration: agent.state === "blocked" ? Infinity : 6000,
        action: {
          label: "Go to pane",
          onClick: () => {
            notify({ type: "read", id: eventId });
            selectAgentRef.current(agent);
          },
        },
      });
    }
  }, [agents]);

  // Clicking a system notification (`send_notification` in lib.rs) brings Wings forward and lands here.
  useEffect(() => {
    const unlisten = api.onFocusPane((paneId) => {
      const agent = agentsRef.current.find((a) => a.paneId === paneId);
      if (agent) selectAgentRef.current(agent);
    });
    return () => void unlisten.then((off) => off());
  }, []);

  // macOS never asks before keeping another app's data, like Chrome's, from a program in a pane, so Wings asks.
  useEffect(() => {
    const waiting = new Map<string, string>();
    const recheck = () => {
      for (const [folder, name] of waiting) {
        void api.privacyCheck(folder).then((ok) => {
          if (!ok || !waiting.delete(folder)) return;
          toast.dismiss(`privacy-${folder}`);
          toast.success(`Wings can read ${name}'s data now`);
        });
      }
    };
    window.addEventListener("focus", recheck);
    const unlisten = api.onPrivacyBlocked(({ folder, name, program }) => {
      waiting.set(folder, name);
      toast(`macOS kept ${name}'s data from ${program}`, {
        id: `privacy-${folder}`,
        description: `Turn on ${name} for Wings in Privacy & Security, Files & Folders. macOS doesn't ask for this one.`,
        duration: Infinity,
        action: { label: "Open Settings", onClick: () => void api.privacyOpenSettings() },
      });
    });
    return () => {
      window.removeEventListener("focus", recheck);
      void unlisten.then((u) => u());
    };
  }, []);

  useEffect(() => startUpdateChecks(), []);

  useEffect(() => {
    const host = new PluginHost({
      setBadge: (pluginId, paneId, badge) => setBadges((all) => withBadge(all, paneId, pluginId, badge)),
      setPaneStatus: (pluginId, paneId, status) => setStatuses((all) => withBadge(all, paneId, pluginId, status)),
      setPaneRail: (pluginId, paneId, rail) => setRails((all) => withBadge(all, paneId, pluginId, rail)),
      openDiff: setDiff,
      clearPlugin: (pluginId) => {
        setRails((all) =>
          Object.fromEntries(Object.entries(all).map(([paneId, byPlugin]) => [paneId, Object.fromEntries(Object.entries(byPlugin).filter(([id]) => id !== pluginId))])),
        );
        setStatuses((all) =>
          Object.fromEntries(Object.entries(all).map(([paneId, byPlugin]) => [paneId, Object.fromEntries(Object.entries(byPlugin).filter(([id]) => id !== pluginId))])),
        );
        setBadges((all) =>
          Object.fromEntries(Object.entries(all).map(([paneId, byPlugin]) => [paneId, Object.fromEntries(Object.entries(byPlugin).filter(([id]) => id !== pluginId))])),
        );
        setSidebarLabels((all) => Object.fromEntries(Object.entries(all).filter(([key]) => !key.startsWith(`${pluginId}:`))));
        setProjectSections((all) => withoutPlugin(all, pluginId));
        // Its sidebar frames are gone too; a restart gets a new key so the page loads again.
        setEpochs((all) => ({ ...all, [pluginId]: (all[pluginId] ?? 0) + 1 }));
        setModal((key) => (key?.startsWith(`${pluginId}:`) ? null : key));
      },
      navigate: (direction) => runCommand(direction === "back" ? "go-back" : "go-forward"),
      showView: (pluginId, viewId, place, options) => {
        const key = `${pluginId}:${viewId}`;
        if (place === "modal") return setModal(key);
        if (place === "tab") {
          const spaceId = activeSpaceIdRef.current;
          if (!spaceId) throw new Error("A view opens as a tab in the project on screen, and no project is open");
          return showViewTab(spaceId, pluginId, viewId, options);
        }
        setRightSidebar(key);
        setOpenedSidebars((all) => (all.includes(key) ? all : [...all, key]));
      },
      notified: (pluginName, title, body, plugin) => {
        notificationSound();
        notify({ type: "add", event: { id: `plugin:${pluginName}:${Date.now()}`, at: Date.now(), kind: "plugin", title: title || pluginName, project: pluginName, detail: body, paneId: null, read: false, plugin } });
      },
      openSettings: (pluginId) => {
        setSettingsSection(pluginSettingsSection(pluginId));
        setSettingsOpen(true);
      },
      hideView: (pluginId, viewId) => {
        const key = `${pluginId}:${viewId}`;
        setModal((open) => (open === key ? null : open));
        setRightSidebar((open) => (open === key ? null : open));
        setOpenedSidebars((all) => all.filter((k) => k !== key));
        closeTabs(tabsRef.current.flatMap((t) => (isViewTab(t) && t.view.pluginId === pluginId && t.view.viewId === viewId ? [t.id] : [])));
      },
      setSidebarLabel: (pluginId, sidebarId, label) =>
        setSidebarLabels((all) => withLabel(all, `${pluginId}:${sidebarId}`, label)),
      updateDiff: (pluginId, id, update) =>
        setDiff((d) => (d && d.id === id && d.pluginId === pluginId ? { ...d, ...update } : d)),
      currentProject: () => activeSpaceIdRef.current,
      projectOf: (spaceId) => {
        const space = spacesRef.current.find((s) => s.id === spaceId);
        return space ? pluginProject(space) : null;
      },
      projects: () => spacesRef.current.map(pluginProject),
      setProjectSection: (pluginId, projectId, sectionId, rows) => setProjectSections((all) => withSection(all, projectId, pluginId, sectionId, rows)),
      tabState: (tabId) => {
        const tab = tabsRef.current.find((t) => t.id === tabId);
        return tab && isViewTab(tab) ? tab.state : undefined;
      },
      setTabState: (tabId, state) => {
        const set = (ts: Tab[]) => ts.map((t) => (t.id === tabId && isViewTab(t) ? { ...t, state } : t));
        // The ref only follows state on the next render, so write it now: a `tabState()` right after sees this value.
        tabsRef.current = set(tabsRef.current);
        setTabs(set);
      },
      openPane: async ({ spaceId, cwd, input, placement }) => {
        let key: string | undefined;
        if (placement === "tab") key = openTab(spaceId, input, null, cwd);
        else {
          const tab = tabsRef.current.find((t) => t.id === activeTabIdRef.current);
          if (!tab || !isPaneTab(tab)) throw new Error('There is no pane to split. Use placement "tab".');
          // A tab holds one project's panes.
          if (tab.spaceId !== spaceId) throw new Error("A split opens beside the focused pane, so its cwd must be in the project on screen");
          key = splitPane(tab.focusedPane, placement === "right" ? "row" : "column", input, cwd);
        }
        const paneId = key && (await terminals.get(key)?.whenStarted);
        if (!paneId) throw new Error("The pane didn't start");
        return paneId;
      },
      focusPane: (paneId) => {
        const key = Object.keys(panesRef.current).find((k) => panesRef.current[k].paneId === paneId);
        const tab = key && tabOfPane(tabsRef.current, key);
        if (!key || !tab) throw new Error(`no pane ${paneId}`);
        setActiveSpaceId(tab.spaceId);
        setActiveTabBySpace((m) => ({ ...m, [tab.spaceId]: tab.id }));
        // A pane zoomed over it would keep it hidden.
        updateTab(tab.id, (t) => ({ focusedPane: key, zoomedPane: t.zoomedPane === key ? key : null }));
      },
    });
    pluginHost.current = host;
    void api.pluginsList().then((list) => {
      setPlugins(list);
      setPluginsLoaded(true);
      host.sync(list);
    });
    const unlisten = api.onPluginsChanged(async ({ id, review }) => {
      const list = await api.pluginsList();
      setPlugins(list);
      host.sync(list, id);
      if (review) {
        setReviewPlugin(id);
        openPlugins(id);
      }
    });
    return () => {
      host.dispose();
      void unlisten.then((off) => off());
    };
  }, []);

  // Pages that asked for their project hear when it switches or its branch or name changes.
  useEffect(() => {
    pluginHost.current?.publishProjects();
    pluginHost.current?.publishProjectList();
  }, [spaces, git, activeSpaceId]);

  // Plugins see every pane (with its agent's session, if any); badges go away with their pane.
  useEffect(() => {
    const list = Object.values(panes).flatMap((meta) => {
      if (!meta.paneId) return [];
      const space = spaces.find((s) => s.id === meta.spaceId);
      const agent = agentsByPane.get(meta.paneId);
      const info = paneInfo[meta.paneId];
      return [
        {
          paneId: meta.paneId,
          cwd: info?.cwd ?? space?.path ?? "",
          command: info?.command ?? "",
          project: space?.name ?? "",
          projectId: meta.spaceId,
          session: agent?.sessionId ? { sessionId: agent.sessionId, kind: agent.kind, name: agent.name, state: agent.state, startedAt: agent.startedAt } : null,
          focused: meta.paneId === focusedPaneId,
        },
      ];
    });
    pluginHost.current?.publishPanes(list);
    const live = new Set(list.map((p) => p.paneId));
    setBadges((all) => {
      const kept = Object.fromEntries(Object.entries(all).filter(([paneId]) => live.has(paneId)));
      return Object.keys(kept).length === Object.keys(all).length ? all : kept;
    });
    setStatuses((all) => {
      const kept = Object.fromEntries(Object.entries(all).filter(([paneId]) => live.has(paneId)));
      return Object.keys(kept).length === Object.keys(all).length ? all : kept;
    });
    setRails((all) => {
      const kept = Object.fromEntries(Object.entries(all).filter(([paneId]) => live.has(paneId)));
      return Object.keys(kept).length === Object.keys(all).length ? all : kept;
    });
  }, [panes, agentsByPane, paneInfo, spaces, focusedPaneId]);

  const paneBadges = useMemo(() => {
    const out: Record<string, { pluginId: string; badge: Badge }[]> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const byPlugin = meta.paneId ? badges[meta.paneId] : undefined;
      if (byPlugin) out[key] = Object.entries(byPlugin)
          .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)) // Stable by plugin id, whatever order they last set theirs in.
          .map(([pluginId, badge]) => ({ pluginId, badge }));
    }
    return out;
  }, [panes, badges]);

  const paneStatuses = useMemo(() => {
    const out: Record<string, PaneStatusEntry[]> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const byPlugin = meta.paneId ? statuses[meta.paneId] : undefined;
      // Plugin order, the order the Plugins list shows them in.
      const entries = byPlugin ? activePlugins.flatMap((p) => (byPlugin[p.id] ? [{ pluginId: p.id, status: byPlugin[p.id] }] : [])) : [];
      if (entries.length) out[key] = entries;
    }
    return out;
  }, [panes, statuses, activePlugins]);

  const paneRails = useMemo(() => {
    const out: Record<string, PaneRailEntry[]> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const byPlugin = meta.paneId ? rails[meta.paneId] : undefined;
      const entries = byPlugin ? activePlugins.flatMap((p) => (byPlugin[p.id] ? [{ pluginId: p.id, rail: byPlugin[p.id] }] : [])) : [];
      if (entries.length) out[key] = entries;
    }
    return out;
  }, [panes, rails, activePlugins]);

  const onOpenStatus = useCallback((pluginId: string, page: string) => void pluginHost.current?.openStatusTarget(pluginId, page).catch((e) => toast.error(String(e))), []);

  const onBadgeAction = useCallback(async (pluginId: string, paneKey: string, actionId: string) => {
    const paneId = panesRef.current[paneKey]?.paneId;
    if (paneId && pluginHost.current) await pluginHost.current.sendAction(pluginId, paneId, actionId);
  }, []);

  async function addSpace(path: string) {
    const space = await api.spacesAdd(path);
    const added = !spacesRef.current.some((s) => s.id === space.id);
    setSpaces((list) => (list.some((s) => s.id === space.id) ? list : [...list, space]));
    if (added && firstTabFor(newProjectOpensRef.current, true) === "project") showProjectPage(space.id);
    else selectSpace(space.id);
  }

  /** Opens a past session where it started, in the project that holds its folder. A new project is added. */
  async function launchSession(session: { id: string; cwd: string; kind: SessionKind }, fork: boolean) {
    const line = fork ? forkLine(session.kind, session.id) : resumeLine(session.kind, session.id);
    if (!isSessionIdOf(session.kind, session.id) || !line) return;
    const holders = spaces.filter((s) => session.cwd === s.path || session.cwd.startsWith(`${s.path}/`));
    const space = holders.sort((a, b) => b.path.length - a.path.length)[0] ?? (await api.spacesAdd(session.cwd));
    setSpaces((list) => (list.some((s) => s.id === space.id) ? list : [...list, space]));
    const cd = session.cwd === space.path ? "" : `cd ${shellQuote(session.cwd)} && `;
    openTab(space.id, cd + line, fork ? null : session.id, null, session.kind);
  }

  /** Runs the agent's resume command at the prompt of the open tab's focused pane, after a `cd` if it isn't in the session's folder. */
  function resumeHere(session: { id: string; cwd: string; kind: SessionKind }) {
    const tab = tabsRef.current.find((t) => t.id === activeTabIdRef.current);
    const key = tab && isPaneTab(tab) ? tab.focusedPane : undefined;
    const paneId = key ? panesRef.current[key]?.paneId : null;
    // The line is typed into a terminal, where a control character acts as a key press.
    if (!key || !paneId || !isSessionIdOf(session.kind, session.id) || /[\x00-\x1f\x7f]/.test(session.cwd) || !isShellCommand(paneInfo[paneId]?.command)) return;
    const cd = session.cwd === paneInfo[paneId]?.cwd ? "" : `cd ${shellQuote(session.cwd)} && `;
    void api.paneWrite(paneId, cd + resumeLine(session.kind, session.id)).catch((e) => toast.error("Couldn't type the resume command", { description: String(e) }));
    setHistoryOpen(false);
    // The sheet hands focus back to what opened it as it closes, so take it after that.
    setTimeout(() => terminals.get(key)?.focus(), 0);
  }

  /** Removing a project closes its tabs, running agents included, so one with tabs open asks first. */
  const [removeAsk, setRemoveAsk] = useState<{ id: string; name: string; tabs: number } | null>(null);
  function askRemoveSpace(spaceId: string) {
    const tabs = tabsRef.current.filter((t) => t.spaceId === spaceId).length;
    if (tabs === 0) return void removeSpace(spaceId).catch((e) => toast.error("Couldn't remove the project", { description: String(e) }));
    setRemoveAsk({ id: spaceId, name: spaces.find((s) => s.id === spaceId)?.name ?? "this project", tabs });
  }

  async function removeSpace(spaceId: string) {
    closeTabs(tabsRef.current.flatMap((t) => (t.spaceId === spaceId ? [t.id] : [])));
    await api.spacesRemove(spaceId);
    const rest = spaces.filter((s) => s.id !== spaceId);
    setSpaces(rest);
    setProjectSections((all) => withoutProject(all, spaceId));
    setCollapsedSections((all) => withoutProjectCollapsed(all, spaceId));
    if (activeSpaceId === spaceId) {
      if (rest[0]) selectSpace(rest[0].id);
      else setActiveSpaceId(null);
    }
  }

  function selectAgent(agent: Agent) {
    const key = Object.keys(panes).find((k) => panes[k].paneId === agent.paneId);
    if (key) selectPaneKey(key);
  }

  function selectPaneKey(key: string) {
    const tab = tabOfPane(tabs, key);
    if (!tab) return;
    setActiveSpaceId(tab.spaceId);
    setActiveTabBySpace((m) => ({ ...m, [tab.spaceId]: tab.id }));
    updateTab(tab.id, () => ({ focusedPane: key }));
  }

  function forkAgent(agent: Agent) {
    const line = agent.sessionId && isSessionIdOf(agent.kind, agent.sessionId) ? forkLine(agent.kind, agent.sessionId) : null;
    if (line) openTab(agent.spaceId, line);
  }

  function closeAgent(agent: Agent) {
    const key = Object.keys(panes).find((k) => panes[k].paneId === agent.paneId);
    if (key) closePane(key);
  }

  const patchPane = (key: string, patch: Partial<PaneMeta>) => setPanes((p) => (p[key] ? { ...p, [key]: { ...p[key], ...patch } } : p));

  /**
   * Brings a pane's session back: a pane not started yet starts with the resume line, a started one gets it typed at
   * its prompt. Its row shows "starting" from here until the agent is seen, or "couldn't resume" after
   * RESUME_TIMEOUT_MS or when the pane isn't at a shell prompt.
   */
  function resumePane(key: string, focus = true) {
    const meta = panesRef.current[key];
    if (!meta?.sessionId) return;
    // Already on its way: go to the pane, and don't type the line again.
    if (meta.resumeAt !== null && !meta.resumeFailed) return focus ? selectPaneKey(key) : undefined;
    const shellReady = !!meta.paneId && isShellCommand(paneInfoRef.current[meta.paneId]?.command);
    if (meta.paneId && !shellReady) return patchPane(key, { ...noResume, resumeAt: Date.now(), resumeFailed: true });
    patchPane(key, { ...noResume, resumeAt: Date.now() });
    if (meta.paneId) void api.paneSend(meta.paneId, resumeLine(meta.kind, meta.sessionId, meta.args)).catch((e) => toast.error("Couldn't restart the agent", { description: String(e) }));
    if (focus) selectPaneKey(key);
  }
  const resumePaneRef = useRef(resumePane);
  resumePaneRef.current = resumePane;

  /** Ends the agent in a pane with its own exit command (`/exit` in Claude Code and opencode), never by killing it. */
  function sleepPane(key: string) {
    const paneId = panesRef.current[key]?.paneId;
    if (!paneId) return;
    patchPane(key, { asleep: true });
    // The slash command menu opens as `/exit` is typed, so Enter comes a moment later as its own key press.
    void api.paneSend(paneId, "/exit").catch((e) => console.error(e));
    setTimeout(() => void api.paneSend(paneId, "\r").catch((e) => console.error(e)), 200);
    // If the agent hasn't left, or something else took its place, the pane isn't asleep.
    setTimeout(() => {
      const stillThere = agentsRef.current.some((a) => a.paneId === paneId) || !isShellCommand(paneInfoRef.current[paneId]?.command);
      if (!stillThere) return;
      // It didn't take /exit: leave this pane awake from now on instead of typing into it again.
      sleepRefused.current.add(key);
      patchPane(key, { asleep: false });
    }, EXIT_GRACE_MS);
  }
  const sleepPaneRef = useRef(sleepPane);
  sleepPaneRef.current = sleepPane;

  /**
   * The last check before ending an agent whose clock ran out: one read of its process children, which says whether
   * a command it started (a dev server in the background, say) is still running. If so the agent stays awake and
   * its clock starts over. Nothing polls this; it happens once per deadline.
   */
  const sleepChecking = useRef(new Set<string>());
  async function sleepIfQuiet(key: string) {
    if (sleepChecking.current.has(key)) return;
    sleepChecking.current.add(key);
    try {
      const meta = panesRef.current[key];
      const agent = meta?.paneId ? agentsRef.current.find((a) => a.paneId === meta.paneId) : undefined;
      if (!meta || !agent) return;
      // If the check itself fails, the agent is treated as busy.
      const background = await api.agentBusy(agent.pid).catch(() => true);
      const now = panesRef.current[key];
      const visible = activeTabIdRef.current ? paneIds(tabsRef.current.find((t): t is PaneTab => t.id === activeTabIdRef.current && isPaneTab(t))?.layout ?? pane("")).includes(key) : false;
      const current = now?.paneId ? agentsRef.current.find((a) => a.paneId === now.paneId) : undefined;
      if (now && canSleep({ agent: current, visible, draft: !!now.paneId && paneInput.hasDraft(now.paneId), busy: now.asleep || now.resumeAt !== null, background })) sleepPane(key);
      else {
        sleepSince.current.set(key, Date.now());
        setSleepTick((n) => n + 1);
      }
    } finally {
      sleepChecking.current.delete(key);
    }
  }
  const sleepIfQuietRef = useRef(sleepIfQuiet);
  sleepIfQuietRef.current = sleepIfQuiet;

  // The pane keys on screen. Opening one that holds a sleeping agent wakes it.
  const visibleKeys = activeTab ? paneIds(activeTab.layout).join(" ") : "";
  useEffect(() => {
    for (const key of visibleKeys.split(" ")) {
      const meta = panes[key];
      if (meta?.asleep && meta.paneId && meta.resumeAt === null && !agentsByPane.has(meta.paneId)) resumePaneRef.current(key, false);
    }
  }, [visibleKeys, panes, agentsByPane]);

  // Sleep: panes that stay eligible for `sleep.minutes` are put to sleep. It runs when agents, panes or the visible tab
  // change, and sets one timer for the earliest deadline: nothing polls, the detection ticks already report every change.
  const sleepSince = useRef(new Map<string, number>());
  const sleepRefused = useRef(new Set<string>());
  useEffect(() => {
    if (!sleep.enabled) {
      sleepSince.current = new Map();
      return;
    }
    const visible = new Set(visibleKeys.split(" "));
    const eligible = Object.entries(panes)
      .filter(([key, meta]) => !sleepRefused.current.has(key) && canSleep({ agent: meta.paneId ? agentsByPane.get(meta.paneId) : undefined, visible: visible.has(key), draft: !!meta.paneId && paneInput.hasDraft(meta.paneId), busy: meta.asleep || meta.resumeAt !== null, background: false }))
      .map(([key]) => key);
    const now = Date.now();
    const plan = planSleep(sleepSince.current, eligible, now, sleep.minutes);
    sleepSince.current = plan.since;
    for (const key of plan.due) void sleepIfQuietRef.current(key);
    if (plan.nextAt === null) return;
    // A fresh render pass at the deadline: the state is the same, the clock has moved.
    const timer = setTimeout(() => setSleepTick((n) => n + 1), Math.max(0, plan.nextAt - now));
    return () => clearTimeout(timer);
  }, [agentsByPane, panes, visibleKeys, sleep, sleepTick]);

  // A resume that shows no agent in time becomes "couldn't resume", and stays in the list for a retry.
  useEffect(() => {
    const at = nextResumeDeadline(Object.values(panes));
    if (at === null) return;
    const timer = setTimeout(
      () =>
        setPanes((prev) => {
          const now = Date.now();
          const expired = Object.entries(prev).filter(([, m]) => m.resumeAt !== null && !m.resumeFailed && resumeExpired(m.resumeAt, now));
          return expired.length ? { ...prev, ...Object.fromEntries(expired.map(([k, m]) => [k, { ...m, resumeFailed: true }])) } : prev;
        }),
      Math.max(0, at - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [panes]);

  /** Each tab's title: your name, or the automatic one with what tells it from a tab that would read the same. */
  const shown = tabTitles(spaceTabs.flatMap((t) => (isPaneTab(t) ? [{ id: t.id, name: t.name, title: labels[t.focusedPane]?.title ?? "", detail: labels[t.focusedPane]?.detail ?? "" }] : [])));
  const tabViews: TabView[] = spaceTabs.map((t) => {
    if (isProjectTab(t)) return { id: t.id, project: { title: "Project page" } };
    if (isViewTab(t)) {
      const plugin = activePlugins.find((p) => p.id === t.view.pluginId);
      const page = plugin && pagesOf(plugin).find((v) => v.id === t.view.viewId);
      return { id: t.id, view: { title: page?.title ?? t.view.viewId, icon: page?.icon ?? "", iconSvg: page?.iconSvg } };
    }
    return {
      id: t.id,
      label: labels[t.focusedPane],
      title: shown[t.id],
      named: !!t.name,
      layout: t.layout,
      focusedPane: t.focusedPane,
      state: rollUp(paneIds(t.layout).flatMap((key) => labels[key]?.agent?.state ?? [])),
    };
  });

  const toggleSection = useCallback((projectId: string, key: string) => setCollapsedSections((all) => toggleCollapsed(all, collapseKey(projectId, key))), []);
  /** A row under a project in the sidebar was clicked: its page opens as a tab in that project. */
  const openSectionRow = useCallback(
    (projectId: string, pluginId: string, open: { view: string; args?: unknown }, newTab?: boolean) => {
      const view = pluginsRef.current.find((p) => p.id === pluginId)?.contributes.views.find((v) => v.id === open.view && v.opens.includes("tab"));
      if (view) showViewTab(projectId, pluginId, view.id, { multiple: view.multiple === true, args: open.args, newTab });
    },
    [showViewTab],
  );
  const sidebarProps: Omit<ComponentProps<typeof Sidebar>, "open"> = {
    spaces,
    git,
    onRefreshGit: () => api.gitRefresh().then(setGit),
    agents,
    activeSpaceId,
    focusedPaneId,
    onSelectSpace: selectSpace,
    onSelectAgent: selectAgent,
    pending,
    onSelectPending: resumePane,
    paneKeyOf: Object.fromEntries(Object.entries(panes).flatMap(([key, meta]) => (meta.paneId ? [[meta.paneId, key]] : []))),
    paneOrder: Object.fromEntries(Object.keys(panes).map((key, i) => [key, i])),
    onNewTab: (id) => openTab(id),
    onOpenProjectPage: showProjectPage,
    projectSections,
    plugins: activePlugins,
    collapsedSections,
    onToggleSection: toggleSection,
    onOpenSectionRow: openSectionRow,
    onForkAgent: forkAgent,
    onCloseAgent: closeAgent,
    onAddSpace: (path) => void addSpace(path),
    onRemoveSpace: askRemoveSpace,
    onSearch: () => setPaletteOpen(true),
    onHistory: () => setHistoryOpen(true),
    onSettings: () => setSettingsOpen(true),
    notifications: {
      events: notifications,
      onOpen: () => notify({ type: "readAll" }),
      onClear: () => notify({ type: "clear" }),
      onGoToPane: (paneId) => {
        const agent = agentsRef.current.find((a) => a.paneId === paneId);
        if (agent) selectAgent(agent);
        else toast("That pane has closed", { duration: 2000 });
      },
      onButton: (plugin, button) => {
        if ("url" in button) return void api.pluginOpenUrl(plugin.id, button.url).catch((e) => toast.error(String(e)));
        const projectId = plugin.projectId ?? activeSpaceIdRef.current;
        if (!projectId) return void toast("Open a project first", { duration: 2000 });
        openSectionRow(projectId, plugin.id, button.open, true);
      },
    },
    onMoveSpace: (id, slot) => setSpaces((list) => reorder(list, list.findIndex((s) => s.id === id), list.findIndex((s) => s.id === slot))),
    sort: projectSort,
    onSortChange: setProjectSort,
    lastActive,
    olderOpen,
    onToggleOlder: () => setOlderOpen((open) => !open),
    onTogglePin: (id, pinned) => {
      setSpaces((list) => list.map((s) => (s.id === id ? { ...s, pinned } : s)));
      void api.spacesSetPinned(id, pinned).catch((e) => toast.error("Couldn't pin the project", { description: String(e) }));
    },
  };

  const editors = useEditors();
  const [editorId] = useDefaultEditorId();
  const editor = defaultEditor(editors, editorId);
  const closeFocused = () => {
    const tab = tabsRef.current.find((t) => t.id === activeTabIdRef.current);
    if (tab && !isPaneTab(tab)) closeTab(tab.id);
    else if (tab) closePane(tab.focusedPane);
  };
  const focusToward = (dir: "left" | "right" | "up" | "down") => {
    const next = activeTab && neighborPane(activeTab.focusedPane, dir);
    if (next) focusPane(next);
  };
  const moveActiveTab = (by: -1 | 1) => {
    const at = spaceTabs.findIndex((t) => t.id === activeTabId);
    if (at >= 0) moveTabTo(activeTabId!, at + by);
  };
  const stepTab = (by: -1 | 1) => {
    const at = spaceTabs.findIndex((t) => t.id === activeTabId);
    const tab = spaceTabs[(at + by + spaceTabs.length) % spaceTabs.length];
    if (activeSpaceId && tab) setActiveTabBySpace((m) => ({ ...m, [activeSpaceId]: tab.id }));
  };
  const jumpToAgent = (back: boolean) => {
    const next = nextNeedingYou(agents, focusedPaneId, back);
    if (next) selectAgent(next);
    else toast("No agent needs you", { id: "no-agent-needs-you", duration: 2000 });
  };
  const commands = createCommands({
    project: activeSpaceId !== null,
    terminalTab: activeTab !== null,
    projectPage: shownTab !== null && isProjectTab(shownTab),
    tabs: spaceTabs.length,
    canGoBack: canBack,
    canGoForward: canForward,
    dev: import.meta.env.DEV,
    run: {
      "go-back": () => navigate(goBack),
      "go-forward": () => navigate(goForward),
      "new-tab": () => activeSpaceId && openTab(activeSpaceId),
      "close-tab": () => activeTabId && closeTab(activeTabId),
      "next-tab": () => stepTab(1),
      "previous-tab": () => stepTab(-1),
      "move-tab-left": () => moveActiveTab(-1),
      "move-tab-right": () => moveActiveTab(1),
      "split-right": () => activeTab && splitPane(activeTab.focusedPane, "row"),
      "split-down": () => activeTab && splitPane(activeTab.focusedPane, "column"),
      "close-pane": closeFocused,
      "zoom-pane": () => activeTab && actions.toggleZoom(activeTab.focusedPane),
      "focus-left": () => focusToward("left"),
      "focus-right": () => focusToward("right"),
      "focus-up": () => focusToward("up"),
      "focus-down": () => focusToward("down"),
      "toggle-sidebar": () => setSidebarOpen((o) => !o),
      "next-agent": () => jumpToAgent(false),
      "previous-agent": () => jumpToAgent(true),
      find: () => {
        if (shownTab && isProjectTab(shownTab)) window.dispatchEvent(new CustomEvent(PROJECT_PAGE_FIND, { detail: shownTab.spaceId }));
        else if (activeTab) terminals.get(activeTab.focusedPane)?.openFind();
      },
      palette: () => setPaletteOpen((o) => !o),
      history: () => setHistoryOpen(true),
      "font-up": () => setFontSize((f) => Math.min(24, f + 1)),
      "font-down": () => setFontSize((f) => Math.max(9, f - 1)),
      "font-reset": () => setFontSize(DEFAULT_FONT_SIZE),
      notifications: openNotifications,
      "send-review": openReviewSend,
      "worktree-agent": () => activeSpaceId && requestWorktreeAgent(activeSpaceId),
      ...(editor && { "open-in-editor": () => activeSpaceId && openInEditor(editor, activeSpaceId) }),
      "check-updates": () => void checkForUpdate({ manual: true }),
      settings: () => setSettingsOpen(true),
      reload: reloadUi,
    },
  });
  commandsRef.current = commands;

  /** The pages the + menu offers besides a terminal, and opening one (or a terminal for null) in the project on screen. */
  const newTabs = useMemo(() => newTabEntries(plugins), [plugins]);
  const openNewTab = (entry: NewTabEntry | null, newTab?: boolean) => {
    if (!activeSpace) return;
    if (entry) showViewTab(activeSpace.id, entry.pluginId, entry.viewId, { multiple: entry.multiple, newTab });
    else openTab(activeSpace.id);
  };

  /** What the palette searches besides past sessions. Built only while it is open. */
  const paletteItems = (): PaletteItem[] => {
    const projectName = (id: string) => spaces.find((s) => s.id === id)?.name ?? "";
    const opens = new Set(["history", "settings", "notifications", "send-review", "worktree-agent"]);
    const items: PaletteItem[] = commands
      .filter((c) => c.enabled && c.id !== "palette")
      .map((c) => ({ id: `command:${c.id}`, kind: "command", title: c.title, subtitle: undefined, keywords: c.group, shortcut: shortcutText(c, isMac) || undefined, opensUi: opens.has(c.id), run: c.run }));
    for (const s of spaces) {
      items.push({ id: `project-page:${s.id}`, kind: "command", title: `Project page: ${s.name}`, keywords: "notes facts info", run: () => showProjectPage(s.id) });
    }
    for (const entry of newTabs) {
      items.push({ id: `new-tab:${entry.key}`, kind: "command", title: `New tab: ${entry.title}`, keywords: "plugin page", disabled: !activeSpace, run: () => openNewTab(entry) });
    }
    for (const s of spaces) {
      items.push({ id: `project:${s.id}`, kind: "project", title: s.name, subtitle: s.path.replace(/^\/Users\/[^/]+/, "~"), keywords: s.path, run: () => selectSpace(s.id) });
      items.push({ id: `worktree:${s.id}`, kind: "project", title: `New agent in worktree: ${s.name}`, keywords: "branch", opensUi: true, run: () => requestWorktreeAgent(s.id) });
    }
    for (const t of tabs) {
      const title = isProjectTab(t) ? "Project page" : isViewTab(t) ? (() => { const v = tabViews.find((x) => x.id === t.id); return v && "view" in v ? v.view.title : t.view.viewId; })() : (t.name ?? labels[t.focusedPane]?.title ?? "Terminal");
      items.push({
        id: `tab:${t.id}`,
        kind: "tab",
        title,
        subtitle: projectName(t.spaceId),
        run: () => {
          setActiveSpaceId(t.spaceId);
          setActiveTabBySpace((m) => ({ ...m, [t.spaceId]: t.id }));
        },
      });
    }
    for (const a of agents) {
      items.push({
        id: `agent:${a.paneId}`,
        kind: "agent",
        title: a.name ?? agentLabel(a.kind),
        subtitle: `${projectName(a.spaceId)} · ${a.state === "blocked" ? (a.waitingFor ?? stateLabel.blocked) : stateLabel[a.state]}`,
        state: a.state,
        run: () => selectAgent(a),
      });
    }
    for (const b of pluginButtons) {
      items.push({
        id: `plugin:${b.key}`,
        kind: "plugin",
        title: b.title,
        subtitle: b.pluginId,
        opensUi: true,
        run: () => pressPluginButton(b, new DOMRect(window.innerWidth - 16, 44, 0, 0)),
      });
    }
    return items;
  };
  /** The sessions the palette lists: the latest, or those matching what is typed. */
  const paletteSessions = useCallback(
    async (query: string): Promise<PaletteItem[]> => {
      const found = await api.historySearch(query, {}, 8);
      return found.sessions.flatMap((h) =>
        h.cwd
          ? [{ id: `session:${h.id}`, kind: "session" as const, title: h.title ?? h.firstPrompt ?? "Untitled session", subtitle: [h.cwd.split("/").pop(), h.gitBranch, h.lastActive].filter(Boolean).join(" · "), keywords: h.cwd, run: () => void launchSessionRef.current({ id: h.id, cwd: h.cwd!, kind: h.agent }, false).catch((e) => toast.error("Couldn't resume the session", { description: String(e) })) }]
          : [],
      );
    },
    [],
  );
  const launchSessionRef = useRef(launchSession);
  launchSessionRef.current = launchSession;
  const focusTerminal = useCallback(() => {
    const tab = tabsRef.current.find((t) => t.id === activeTabIdRef.current);
    if (tab && isPaneTab(tab)) terminals.get(tab.focusedPane)?.focus();
  }, []);

  /** What a plugin button on the title bar does; `rect` is the button, which a panel opens under. */
  const pressPluginButton = (button: PluginButton, rect: { right: number; bottom: number }, newTab?: boolean) => {
    if (button.kind === "panel") {
      void api.pluginPanelToggle(button.pluginId, button.id, rect.right, rect.bottom).catch((e) => toast.error("Couldn't open the panel", { description: String(e) }));
      return;
    }
    if (button.kind === "modal") return setModal((open) => (open === button.key ? null : button.key));
    if (button.kind === "tab") return void (activeSpace && showViewTab(activeSpace.id, button.pluginId, button.id, newTab ? { multiple: false, newTab } : undefined));
    if (button.kind === "window") {
      const plugin = activePlugins.find((p) => p.id === button.pluginId);
      if (plugin) void pluginHost.current?.openWindow(plugin, button.id).catch((e) => toast.error(String(e)));
      return;
    }
    setRightSidebar((open) => (open === button.key ? null : button.key));
    setOpenedSidebars((all) => (all.includes(button.key) ? all : [...all, button.key]));
  };
  return (
    <MotionConfig reducedMotion="user">
      <TooltipProvider delay={500}>
        <div ref={frame} className="flex h-full flex-col bg-glass text-foreground">
          <TitleBar
            hasSpace={activeSpace !== null}
            tabs={tabViews}
            activeTabId={activeTabId}
            sidebarOpen={sidebarOpen}
            onToggleSidebar={() => setSidebarOpen((o) => !o)}
            canGoBack={canBack}
            canGoForward={canForward}
            onGoBack={() => navigate(goBack)}
            onGoForward={() => navigate(goForward)}
            onSelect={(id) => activeSpace && setActiveTabBySpace((m) => ({ ...m, [activeSpace.id]: id }))}
            onClose={closeTab}
            onRename={renameTab}
            onMove={moveTabTo}
            newTabEntries={newTabs}
            newAsk={newTabAction(newTabs, newTabPage).kind === "ask"}
            onNew={() => {
              const action = newTabAction(newTabs, newTabPage);
              if (action.kind === "project") activeSpace && showProjectPage(activeSpace.id);
              else openNewTab(action.kind === "page" ? action.entry : null);
            }}
            onNewTerminal={() => openNewTab(null)}
            onNewPage={(entry, newTab) => openNewTab(entry, newTab)}
            onNewProjectPage={() => activeSpace && showProjectPage(activeSpace.id)}
          />
          {/* The peek sits on the row, not in the panes, so it starts at the window's edge, left of the closed sidebar's handle. */}
          <div className="relative min-h-0 flex-1">
          <ResizablePanelGroup
            orientation="horizontal"
            elementRef={panelGroup}
            onLayoutChanged={(_, meta) => {
              // Only a drag counts. While a sidebar slides the panels report in-between sizes, which once saved
              // a 1px sidebar. Under its minimum a sidebar closes, so only real widths are kept.
              if (!meta.isUserInteraction) return;
              const left = leftPanel.current?.getSize().inPixels ?? 0;
              const right = rightPanel.current?.getSize().inPixels ?? 0;
              if (left < SIDES.left.min) setSidebarOpen(false);
              if (right < SIDES.right.min) setRightSidebar(null);
              setWidths((w) => ({
                left: left >= SIDES.left.min ? Math.round(left) : w.left,
                right: right >= SIDES.right.min ? Math.round(right) : w.right,
              }));
            }}
            className="h-full"
          >
            <ResizablePanel
              id="left"
              panelRef={leftPanel}
              collapsible
              collapsedSize={0}
              minSize={SIDES.left.min}
              maxSize={SIDES.left.max}
              defaultSize={SIDES.left.initial}
              groupResizeBehavior="preserve-pixel-size"
              style={PANEL_STYLE}
            >
              <Sidebar open={sidebarOpen} {...sidebarProps} />
            </ResizablePanel>
            <SideHandle disabled={!sidebarOpen} />
            <ResizablePanel id="main" minSize={320} style={PANEL_STYLE}>
              <div ref={mainBox} className="flex h-full min-w-0 flex-col">
              <ProjectBar
                space={activeSpace}
                git={activeSpace ? git[activeSpace.id] : null}
            pluginButtons={pluginButtons}
            pinned={pinnedButtons}
            onTogglePin={(key) => setPinnedButtons((all) => (all.includes(key) ? all.filter((k) => k !== key) : [...all, key]))}
            onPluginButton={pressPluginButton}
            onMountPopover={async (button, container, hidden) => {
              const plugin = activePlugins.find((p) => p.id === button.pluginId);
              if (!plugin || !pluginHost.current) throw new Error(`${button.pluginId} isn't running`);
              return pluginHost.current.mountPage(plugin, button.id, container, "popover", undefined, hidden);
            }}
              />
              <main className="relative min-h-0 min-w-0 flex-1">
                {tabs.map((tab) =>
                  isProjectTab(tab) ? (
                    <ProjectPageTab
                      key={tab.id}
                      space={spaces.find((s) => s.id === tab.spaceId)}
                      branch={git[tab.spaceId]?.branch}
                      plugins={activePlugins}
                      epochs={epochs}
                      host={pluginHost.current}
                      visible={tab.id === activeTabId}
                    />
                  ) : isViewTab(tab) ? (
                    <PluginTab
                      key={tab.id}
                      plugin={activePlugins.find((p) => p.id === tab.view.pluginId)}
                      viewId={tab.view.viewId}
                      pageKey={`${tab.id}:${activePlugins.find((p) => p.id === tab.view.pluginId)?.version}:${epochs[tab.view.pluginId] ?? 0}:${tab.rev ?? 0}`}
                      tab={{ tabId: tab.id, spaceId: tab.spaceId, args: tab.args }}
                      visible={tab.id === activeTabId}
                      loaded={!unloadedViewTabs.has(tab.id)}
                      host={pluginHost.current}
                    />
                  ) : (
                  <PaneGrid
                    key={tab.id}
                    tabId={tab.id}
                    moveTargets={tabViews.flatMap((v) => ("label" in v ? [{ id: v.id, title: v.title }] : []))}
                    layout={tab.layout}
                    visible={tab.id === activeTabId}
                    keepRenderer={recentTabs.current.includes(tab.id)}
                    focusedPane={tab.focusedPane}
                    zoomedPane={tab.zoomedPane}
                    labels={labels}
                    initialInputs={initialInputs}
                    badges={paneBadges}
                    onBadgeAction={onBadgeAction}
                    statuses={paneStatuses}
                    rails={paneRails}
                    onOpenStatus={onOpenStatus}
                    actions={actions}
                  />
                  ),
                )}
                {!activeSpace && (
                  <Welcome
                    paletteKey={shortcutOf("palette", isMac)}
                    jumpKey={shortcutOf("next-agent", isMac)}
                    hotkey={globalHotkey.enabled ? hotkeyLabel(globalHotkey.accelerator, isMac) : null}
                    onAdd={(path) => void addSpace(path)}
                  />
                )}
              </main>
              </div>
            </ResizablePanel>
            <SideHandle disabled={rightRef === null} />
            <ResizablePanel
              id="right"
              panelRef={rightPanel}
              collapsible
              collapsedSize={0}
              minSize={SIDES.right.min}
              maxSize={SIDES.right.max}
              collapsedThreshold={10}
              defaultSize={0}
              groupResizeBehavior="preserve-pixel-size"
              style={PANEL_STYLE}
              className="has-[[data-peek]]:!overflow-visible"
            >
              <RightSidebar
                open={rightRef}
                width={widths.right}
                onPeek={setPeekedSidebar}
                mounted={openedSidebars.flatMap((key) => sidebarRef(key) ?? [])}
                unloaded={unloadedSidebars}
                host={pluginHost.current}
                onSelect={setRightSidebar}
                onCloseTab={(id) => {
                  const rest = openedSidebars.filter((k) => k !== id);
                  setOpenedSidebars(rest);
                  if (rightSidebar === id) setRightSidebar(rest.at(-1) ?? null);
                }}
                onClose={() => setRightSidebar(null)}
                onPopOut={(entry) => {
                  void pluginHost.current?.openWindow(entry.plugin, entry.sidebarId).catch((e) => toast.error(String(e)));
                  const rest = openedSidebars.filter((k) => k !== entry.id);
                  setOpenedSidebars(rest);
                  setRightSidebar(rest.at(-1) ?? null);
                }}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
          {!sidebarOpen && (
            <SidebarPeek width={widths.left}>
              <Sidebar open floating {...sidebarProps} />
            </SidebarPeek>
          )}
          </div>
        </div>
        <DiffViewer
          diff={diff}
          onClose={() => setDiff(null)}
          reviewTargets={(cwd) =>
            reviewTargets({ agents: agentsRef.current, paneInfo, spaces, agentName: (a) => a.name ?? agentLabel(a.kind) }, cwd)
          }
          onSendReview={async (paneId, text, submit) => {
            // Only to a pane still running an agent: in a plain shell the quoted code would run on Enter.
            if (!agentsRef.current.some((a) => a.paneId === paneId)) throw new Error("That agent isn't running any more.");
            await sendToPane(api.paneWrite, paneId, text, submit);
          }}
          onReply={async (view, replyTo, body) => {
            if (!pluginHost.current) throw new Error("Plugins aren't running");
            await pluginHost.current.sendReply(view.pluginId, view.id, replyTo, body);
          }}
        />
        <PluginModal
          open={modalRef}
          host={pluginHost.current}
          onClose={() => setModal(null)}
          onPopOut={(entry) => {
            void pluginHost.current?.openWindow(entry.plugin, entry.viewId).catch((e) => toast.error(String(e)));
            setModal(null);
          }}
        />
        <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} items={paletteOpen ? paletteItems() : []} loadSessions={paletteSessions} focusTerminal={focusTerminal} />
        <TerminalCommandPrompt />
        <Toaster position="bottom-right" />
        <UpdateDialog />
        <Dialog open={removeAsk !== null} onOpenChange={(open) => !open && setRemoveAsk(null)}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>Remove {removeAsk?.name}?</DialogTitle>
              <DialogDescription>
                {removeAsk?.tabs === 1 ? "Its open tab" : `Its ${removeAsk?.tabs} open tabs`} will close, and any agent running there stops. The folder stays on disk.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="ghost" onClick={() => setRemoveAsk(null)}>
                Keep
              </Button>
              <Button
                variant="destructive"
                onClick={() => {
                  const id = removeAsk?.id;
                  setRemoveAsk(null);
                  if (id) void removeSpace(id).catch((e) => toast.error("Couldn't remove the project", { description: String(e) }));
                }}
              >
                Remove
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
        <WorktreeAgentDialog
          spaces={spaces}
          onCreated={(space, input) => {
            setSpaces((list) => (list.some((s) => s.id === space.id) ? list : [...list, space]));
            openTab(space.id, input);
          }}
        />
        <SettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          section={shownSection(settingsSection, plugins)}
          onSectionChange={setSettingsSection}
          pluginSettings={settingsPlugins(plugins).map((plugin) => ({ plugin, key: `${plugin.id}:settings:${plugin.version}:${epochs[plugin.id] ?? 0}` }))}
          host={pluginHost.current}
          pluginsProps={{
            plugins,
            selected: settingsPlugin,
            onSelect: setSettingsPlugin,
            dropped: droppedPlugin,
            onDroppedHandled: () => setDroppedPlugin(null),
            review: reviewPlugin,
            onReviewHandled: () => setReviewPlugin(null),
            onChanged: (list, restart) => {
              setPlugins(list);
              pluginHost.current?.sync(list, restart);
            },
            updates: pluginUpdates.updates,
            checking: pluginUpdates.checking,
            onCheck: () => void pluginUpdates.check(true),
          }}
          keys={terminalKeys} onKeysChange={setTerminalKeysState} hotkey={globalHotkey} onHotkeyChange={setGlobalHotkey} sleep={sleep} onSleepChange={setSleep} notifySound={notifySound} onNotifySoundChange={setNotifySound} agentsRunCommands={agentsRunCommands} onAgentsRunCommandsChange={setAgentsRunCommands} newTabs={newTabs} newTabPage={newTabPage} onNewTabPageChange={setNewTabPage} newProjectOpens={newProjectOpens} onNewProjectOpensChange={setNewProjectOpens} look={terminalLook} onLookChange={setTerminalLookState} />
        <ToolInstallDialog
          project={activeSpace}
          paneInfo={paneInfo}
          openTab={(spaceId, input) => terminals.get(openTab(spaceId, input))?.whenStarted ?? Promise.resolve(null)}
          onStarted={() => setSettingsOpen(false)}
          onInstalled={(program) =>
            void api.pluginsList().then((list) => {
              setPlugins(list);
              for (const id of pluginsUsing(list, program)) pluginHost.current?.sync(list, id);
            })
          }
        />
        <HistorySheet
          space={activeSpace}
          open={historyOpen}
          onOpenChange={setHistoryOpen}
          liveSessionIds={liveSessionIds}
          onResume={({ id, kind }) => {
            setHistoryOpen(false);
            if (activeSpace && isSessionIdOf(kind, id)) openTab(activeSpace.id, resumeLine(kind, id), id, null, kind);
          }}
          pane={focusedPaneId ? { idle: isShellCommand(paneInfo[focusedPaneId]?.command) } : null}
          onResumeHere={resumeHere}
          onLaunch={(session, fork) => {
            setHistoryOpen(false);
            void launchSession(session, fork).catch((e) => toast.error(fork ? "Couldn't fork the session" : "Couldn't resume the session", { description: String(e) }));
          }}
        />
      </TooltipProvider>
    </MotionConfig>
  );
}

/** The edge between a sidebar and the panes: invisible until you point at it. Off while that sidebar is closed. */
function SideHandle({ disabled }: { disabled: boolean }) {
  return (
    <ResizableHandle
      disabled={disabled}
      className="bg-transparent transition-colors duration-150 after:w-2 data-[separator=active]:bg-hairline-strong data-[separator=hover]:bg-hairline-strong data-[separator=disabled]:pointer-events-none"
    />
  );
}
