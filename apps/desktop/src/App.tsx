import { getCurrentWebview } from "@tauri-apps/api/webview";
import { usePanelRef, type PanelImperativeHandle } from "react-resizable-panels";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MotionConfig } from "motion/react";

import { DiffViewer } from "@/components/diff-viewer";
import { HistorySheet } from "@/components/history-sheet";
import { PluginManager } from "@/components/plugin-manager";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { RightSidebar, type SidebarRef } from "@/components/right-sidebar";
import { PaneGrid, type PaneActions } from "@/components/pane-grid";
import { paneLabel, type PaneLabelInfo } from "@/components/pane-label";
import { Sidebar } from "@/components/sidebar";
import { rollUp } from "@/components/status-dot";
import { TitleBar, type PluginButton, type TabView } from "@/components/title-bar";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { TooltipProvider } from "@/components/ui/tooltip";
import { api, type Agent, type GitStatus, type PaneInfo, type PluginView, type Space } from "@/lib/api";
import { PluginHost, type Badge, type DiffView } from "@/lib/plugins";
import { mapPanes, pane, paneIds, remove, setRatio, split, type LayoutNode } from "@/lib/layout";
import { DEFAULT_FONT_SIZE, setTerminalFontSize, shortcutFor, terminals, TerminalSession } from "@/lib/terminal";

type Tab = { id: string; spaceId: string; layout: LayoutNode; focusedPane: string; zoomedPane: string | null };
/** `sessionId` is the Claude session last seen in the pane; `live` once Wings has seen it running this launch. */
type PaneMeta = { spaceId: string; paneId: string | null; sessionId: string | null; live: boolean };

type SavedWorkspace = {
  version: 1;
  activeSpaceId: string | null;
  activeTabBySpace: Record<string, string>;
  sidebarOpen: boolean;
  /** Sidebar widths in pixels, as last dragged. */
  widths?: Widths;
  fontSize: number;
  tabs: Tab[];
  sessions: Record<string, string | null>;
};

type Widths = { left: number; right: number };

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

/** Session ids get typed into a shell, so only accept the UUID shape Claude Code uses. */
const isSessionId = (id: string) => /^[0-9a-f-]{36}$/i.test(id);
const resumeCommand = (sessionId: string) => `claude --resume ${sessionId}\r`;

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
  const [widths, setWidths] = useState<Widths>({ left: 248, right: 360 });
  const leftPanel = usePanelRef();
  const rightPanel = usePanelRef();
  const panelGroup = useRef<HTMLDivElement>(null);
  const placed = useRef(false);
  // Updated on every drag frame; saved into `widths` once the drag ends, so App doesn't re-render per frame.
  const liveWidths = useRef(widths);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [plugins, setPlugins] = useState<PluginView[]>([]);
  const [pluginsOpen, setPluginsOpen] = useState(false);
  const [droppedPlugin, setDroppedPlugin] = useState<string | null>(null);
  /** The plugin sidebar on the right, as `pluginId:sidebarId`, and every one opened so far (kept running). */
  const [rightSidebar, setRightSidebar] = useState<string | null>(null);
  const [openedSidebars, setOpenedSidebars] = useState<string[]>([]);
  const [sidebarLabels, setSidebarLabels] = useState<Record<string, string>>({});
  /** Bumped when a plugin stops, so its sidebar pages mount fresh next time. */
  const [epochs, setEpochs] = useState<Record<string, number>>({});

  const activePlugins = useMemo(() => plugins.filter((p) => p.enabled && p.approved), [plugins]);
  const sidebarRef = useCallback(
    (key: string): SidebarRef | null => {
      const [pluginId, sidebarId] = key.split(":");
      const plugin = activePlugins.find((p) => p.id === pluginId);
      if (!plugin?.contributes.sidebars.some((s) => s.id === sidebarId)) return null;
      return { plugin, sidebarId, id: key, key: `${key}:${plugin.version}:${epochs[pluginId] ?? 0}` };
    },
    [activePlugins, epochs],
  );
  const pluginButtons: PluginButton[] = activePlugins.flatMap((p) => [
    ...p.contributes.sidebars.map((s) => ({
      key: `${p.id}:${s.id}`,
      pluginId: p.id,
      kind: "sidebar" as const,
      id: s.id,
      title: s.title,
      icon: s.icon,
      label: sidebarLabels[`${p.id}:${s.id}`],
      active: rightSidebar === `${p.id}:${s.id}`,
    })),
    ...p.contributes.panels.map((panel) => ({ key: `${p.id}:panel:${panel.id}`, pluginId: p.id, kind: "panel" as const, id: panel.id, title: panel.title, icon: panel.icon })),
  ]);

  // Dropping a .wings-plugin file anywhere on the window opens the manager and installs it.
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const file = event.payload.paths.find((p) => p.endsWith(".wings-plugin"));
      if (!file) return;
      setPluginsOpen(true);
      setDroppedPlugin(file);
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);
  const [ready, setReady] = useState(false);
  const [fontSize, setFontSize] = useState(DEFAULT_FONT_SIZE);
  /** Plugin badges by Rust pane id, then plugin id. */
  const [badges, setBadges] = useState<Record<string, Record<string, Badge>>>({});
  const [diff, setDiff] = useState<DiffView | null>(null);
  const pluginHost = useRef<PluginHost | null>(null);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const panesRef = useRef(panes);
  panesRef.current = panes;

  const activeSpace = spaces.find((s) => s.id === activeSpaceId) ?? null;
  const spaceTabs = tabs.filter((t) => t.spaceId === activeSpaceId);
  const activeTabId = activeSpaceId ? (activeTabBySpace[activeSpaceId] ?? spaceTabs[0]?.id ?? null) : null;
  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;
  const focusedPaneId = activeTab ? (panes[activeTab.focusedPane]?.paneId ?? null) : null;
  const agentsByPane = useMemo(() => new Map(agents.map((a) => [a.paneId, a])), [agents]);
  const liveSessionIds = useMemo(() => new Set(agents.flatMap((a) => (a.sessionId ? [a.sessionId] : []))), [agents]);

  const labels = useMemo(() => {
    const out: Record<string, PaneLabelInfo> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const agent = meta.paneId ? agentsByPane.get(meta.paneId) : undefined;
      const info = meta.paneId ? paneInfo[meta.paneId] : undefined;
      out[key] = paneLabel(agent, info, spaces.find((s) => s.id === meta.spaceId));
    }
    return out;
  }, [panes, agentsByPane, paneInfo, spaces]);

  const updateTab = useCallback((tabId: string, patch: (t: Tab) => Partial<Tab>) => {
    setTabs((ts) => ts.map((t) => (t.id === tabId ? { ...t, ...patch(t) } : t)));
  }, []);

  const focusPane = useCallback((key: string) => {
    const tab = tabsRef.current.find((t) => paneIds(t.layout).includes(key));
    if (tab && tab.focusedPane !== key) updateTab(tab.id, () => ({ focusedPane: key }));
  }, [updateTab]);

  /** Creates the terminal for a new pane; it starts its shell once its card mounts. */
  const createPane = useCallback(
    (spaceId: string, initialInput: string | null, sessionId: string | null = null) => {
      const key = crypto.randomUUID();
      terminals.set(
        key,
        new TerminalSession(spaceId, {
          onFocus: () => focusPane(key),
          onStarted: (paneId) => setPanes((p) => ({ ...p, [key]: { ...p[key], paneId } })),
        }),
      );
      setPanes((p) => ({ ...p, [key]: { spaceId, paneId: null, sessionId, live: false } }));
      setInitialInputs((m) => ({ ...m, [key]: initialInput }));
      return key;
    },
    [focusPane],
  );

  const openTab = useCallback(
    (spaceId: string, initialInput: string | null = null, sessionId: string | null = null) => {
      const key = createPane(spaceId, initialInput, sessionId);
      const id = crypto.randomUUID();
      setTabs((ts) => [...ts, { id, spaceId, layout: pane(key), focusedPane: key, zoomedPane: null }]);
      setActiveTabBySpace((m) => ({ ...m, [spaceId]: id }));
      setActiveSpaceId(spaceId);
    },
    [createPane],
  );

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

  const closeTab = useCallback((tabId: string) => {
    const tab = tabsRef.current.find((t) => t.id === tabId);
    if (!tab) return;
    paneIds(tab.layout).forEach(disposePane);
    const rest = tabsRef.current.filter((t) => t.id !== tabId);
    setTabs(rest);
    setActiveTabBySpace((m) => {
      if (m[tab.spaceId] !== tabId) return m;
      const siblings = rest.filter((t) => t.spaceId === tab.spaceId);
      const next = { ...m };
      if (siblings.length) next[tab.spaceId] = siblings[siblings.length - 1].id;
      else delete next[tab.spaceId];
      return next;
    });
  }, []);

  const closePane = useCallback(
    (key: string) => {
      const tab = tabsRef.current.find((t) => paneIds(t.layout).includes(key));
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
    (key: string, dir: "row" | "column") => {
      const tab = tabsRef.current.find((t) => paneIds(t.layout).includes(key));
      if (!tab) return;
      const next = createPane(tab.spaceId, null);
      updateTab(tab.id, (t) => ({ layout: split(t.layout, key, dir, next), focusedPane: next, zoomedPane: null }));
    },
    [createPane, updateTab],
  );

  const actions: PaneActions = useMemo(
    () => ({
      focus: focusPane,
      split: splitPane,
      close: closePane,
      toggleZoom: (key) => {
        const tab = tabsRef.current.find((t) => paneIds(t.layout).includes(key));
        if (tab) updateTab(tab.id, (t) => ({ zoomedPane: t.zoomedPane === key ? null : key, focusedPane: key }));
      },
      resize: (splitId, ratio) => {
        const tab = tabsRef.current.find((t) => t.id === activeTabId);
        if (tab) updateTab(tab.id, (t) => ({ layout: setRatio(t.layout, splitId, ratio) }));
      },
    }),
    [focusPane, splitPane, closePane, updateTab, activeTabId],
  );

  /** Rebuilds saved tabs and splits with fresh shells; Claude panes resume their session. */
  const restore = useCallback(
    (list: Space[], json: string | null) => {
      let saved: SavedWorkspace;
      try {
        saved = json ? JSON.parse(json) : null;
      } catch {
        return false;
      }
      if (saved?.version !== 1) return false;
      // Before any terminal exists: a later font change resizes every pane right after its first draw,
      // which leaves Claude Code's screen garbled until it repaints.
      if (saved.fontSize) {
        setTerminalFontSize(saved.fontSize);
        setFontSize(saved.fontSize);
      }
      const known = new Set(list.map((s) => s.id));
      const restored = saved.tabs
        .filter((t) => known.has(t.spaceId))
        .map((t) => {
          const keys: Record<string, string> = {};
          const layout = mapPanes(t.layout, (old) => {
            const session = saved.sessions[old];
            const resume = session && isSessionId(session) ? session : null;
            keys[old] = createPane(t.spaceId, resume && resumeCommand(resume), resume);
            return keys[old];
          });
          const focusedPane = keys[t.focusedPane] ?? paneIds(layout)[0];
          return { ...t, layout, focusedPane, zoomedPane: t.zoomedPane ? (keys[t.zoomedPane] ?? null) : null };
        });
      if (!restored.length) return false;
      const tabIds = new Set(restored.map((t) => t.id));
      setTabs(restored);
      setActiveTabBySpace(Object.fromEntries(Object.entries(saved.activeTabBySpace).filter(([, id]) => tabIds.has(id))));
      setSidebarOpen(saved.sidebarOpen);
      if (saved.widths) setWidths((liveWidths.current = saved.widths));
      setActiveSpaceId(saved.activeSpaceId && known.has(saved.activeSpaceId) ? saved.activeSpaceId : restored[0].spaceId);
      return true;
    },
    [createPane],
  );

  // Initial load and live events from the Rust core.
  useEffect(() => {
    void (async () => {
      await api.panesReset();
      const [list, saved] = await Promise.all([api.spacesList(), api.workspaceLoad()]);
      setSpaces(list);
      if (!restore(list, saved) && list[0]) selectSpace(list[0].id);
      setReady(true);
    })();
    void api.agentsList().then(setAgents);
    void api.paneInfo().then(setPaneInfo);
    void api.gitStatus().then(setGit);
    const unlisten = [
      api.onAgents(setAgents),
      api.onPaneInfo(setPaneInfo),
      api.onGitStatus(setGit),
      api.onPaneExited((paneId) => {
        const key = Object.keys(panesRef.current).find((k) => panesRef.current[k].paneId === paneId);
        if (key) closePane(key);
      }),
    ];
    return () => unlisten.forEach((p) => void p.then((off) => off()));
  }, [selectSpace, closePane, restore]);

  // Remember which Claude session each pane runs, so a restart can resume it.
  useEffect(() => {
    setPanes((prev) => {
      let next: Record<string, PaneMeta> | null = null;
      for (const [key, meta] of Object.entries(prev)) {
        const agent = meta.paneId ? agentsByPane.get(meta.paneId) : undefined;
        let patch: Partial<PaneMeta> | null = null;
        if (agent?.sessionId && (agent.sessionId !== meta.sessionId || !meta.live)) patch = { sessionId: agent.sessionId, live: true };
        else if (!agent && meta.live) patch = { sessionId: null, live: false };
        if (patch) {
          next ??= { ...prev };
          next[key] = { ...meta, ...patch };
        }
      }
      return next ?? prev;
    });
  }, [agentsByPane]);

  useEffect(() => {
    if (!ready) return;
    const sessions = Object.fromEntries(Object.entries(panes).map(([key, meta]) => [key, meta.sessionId]));
    const saved: SavedWorkspace = { version: 1, activeSpaceId, activeTabBySpace, sidebarOpen, widths, fontSize, tabs, sessions };
    const timer = setTimeout(() => void api.workspaceSave(JSON.stringify(saved)), 400);
    return () => clearTimeout(timer);
  }, [ready, tabs, panes, activeSpaceId, activeTabBySpace, sidebarOpen, widths, fontSize]);

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

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const action = shortcutFor(e);
      if (!action) return;
      e.preventDefault();
      switch (action.kind) {
        case "toggleSidebar":
          return setSidebarOpen((o) => !o);
        case "fontUp":
          return setFontSize((f) => Math.min(24, f + 1));
        case "fontDown":
          return setFontSize((f) => Math.max(9, f - 1));
        case "fontReset":
          return setFontSize(DEFAULT_FONT_SIZE);
      }
      if (!activeSpaceId) return;
      if (action.kind === "newTab") return openTab(activeSpaceId);
      if (action.kind === "tab") {
        const tab = spaceTabs[action.index];
        if (tab) setActiveTabBySpace((m) => ({ ...m, [activeSpaceId]: tab.id }));
        return;
      }
      if (!activeTab) return;
      if (action.kind === "splitRight" || action.kind === "splitDown") {
        return splitPane(activeTab.focusedPane, action.kind === "splitRight" ? "row" : "column");
      }
      if (action.kind === "focus") {
        const next = neighborPane(activeTab.focusedPane, action.dir);
        if (next) focusPane(next);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeSpaceId, activeTab, spaceTabs, openTab, splitPane, focusPane]);

  // The menu owns ⌘W (so it never closes the window); it forwards it here.
  useEffect(() => {
    const unlisten = api.onMenu((id) => {
      if (id !== "close-pane") return;
      const tab = tabsRef.current.find((t) => t.id === activeTabIdRef.current);
      if (tab) closePane(tab.focusedPane);
    });
    return () => void unlisten.then((off) => off());
  }, [closePane]);

  useEffect(() => setTerminalFontSize(fontSize), [fontSize]);

  useEffect(() => {
    const host = new PluginHost({
      setBadge: (pluginId, paneId, badge) =>
        setBadges((all) => {
          if (!badge && !all[paneId]?.[pluginId]) return all;
          const { [pluginId]: _old, ...others } = all[paneId] ?? {};
          return { ...all, [paneId]: badge ? { ...others, [pluginId]: badge } : others };
        }),
      openDiff: setDiff,
      clearPlugin: (pluginId) => {
        setBadges((all) =>
          Object.fromEntries(Object.entries(all).map(([paneId, byPlugin]) => [paneId, Object.fromEntries(Object.entries(byPlugin).filter(([id]) => id !== pluginId))])),
        );
        setSidebarLabels((all) => Object.fromEntries(Object.entries(all).filter(([key]) => !key.startsWith(`${pluginId}:`))));
        // Its sidebar frames are gone too; a restart gets a new key so the page loads again.
        setEpochs((all) => ({ ...all, [pluginId]: (all[pluginId] ?? 0) + 1 }));
      },
      setSidebarLabel: (pluginId, sidebarId, label) =>
        setSidebarLabels(({ [`${pluginId}:${sidebarId}`]: _old, ...rest }) => (label ? { ...rest, [`${pluginId}:${sidebarId}`]: label } : rest)),
      updateDiff: (pluginId, id, update) =>
        setDiff((d) => (d && d.id === id && d.pluginId === pluginId ? { ...d, ...update } : d)),
    });
    pluginHost.current = host;
    void api.pluginsList().then((list) => {
      setPlugins(list);
      host.sync(list);
    });
    return () => host.dispose();
  }, []);

  // Plugins see every pane (with its Claude session, if any); badges go away with their pane.
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
          session: agent?.sessionId ? { sessionId: agent.sessionId, name: agent.name, state: agent.state } : null,
        },
      ];
    });
    pluginHost.current?.publishPanes(list);
    const live = new Set(list.map((p) => p.paneId));
    setBadges((all) => {
      const kept = Object.fromEntries(Object.entries(all).filter(([paneId]) => live.has(paneId)));
      return Object.keys(kept).length === Object.keys(all).length ? all : kept;
    });
  }, [panes, agentsByPane, paneInfo, spaces]);

  const paneBadges = useMemo(() => {
    const out: Record<string, { pluginId: string; badge: Badge }[]> = {};
    for (const [key, meta] of Object.entries(panes)) {
      const byPlugin = meta.paneId ? badges[meta.paneId] : undefined;
      if (byPlugin) out[key] = Object.entries(byPlugin).map(([pluginId, badge]) => ({ pluginId, badge }));
    }
    return out;
  }, [panes, badges]);

  const onBadgeAction = useCallback(async (pluginId: string, paneKey: string, actionId: string) => {
    const paneId = panesRef.current[paneKey]?.paneId;
    if (paneId && pluginHost.current) await pluginHost.current.sendAction(pluginId, paneId, actionId);
  }, []);

  async function addSpace(path: string) {
    const space = await api.spacesAdd(path);
    setSpaces((list) => (list.some((s) => s.id === space.id) ? list : [...list, space]));
    selectSpace(space.id);
  }

  async function removeSpace(spaceId: string) {
    tabsRef.current.filter((t) => t.spaceId === spaceId).forEach((t) => closeTab(t.id));
    await api.spacesRemove(spaceId);
    const rest = spaces.filter((s) => s.id !== spaceId);
    setSpaces(rest);
    if (activeSpaceId === spaceId) {
      if (rest[0]) selectSpace(rest[0].id);
      else setActiveSpaceId(null);
    }
  }

  function selectAgent(agent: Agent) {
    const key = Object.keys(panes).find((k) => panes[k].paneId === agent.paneId);
    const tab = key && tabs.find((t) => paneIds(t.layout).includes(key));
    if (!key || !tab) return;
    setActiveSpaceId(tab.spaceId);
    setActiveTabBySpace((m) => ({ ...m, [tab.spaceId]: tab.id }));
    updateTab(tab.id, () => ({ focusedPane: key }));
  }

  const tabViews: TabView[] = spaceTabs.map((t) => ({
    id: t.id,
    label: labels[t.focusedPane],
    layout: t.layout,
    focusedPane: t.focusedPane,
    state: rollUp(paneIds(t.layout).flatMap((key) => labels[key]?.agent?.state ?? [])),
  }));

  return (
    <MotionConfig reducedMotion="user">
      <TooltipProvider delay={500}>
        <div className="flex h-full flex-col bg-glass text-foreground">
          <TitleBar
            space={activeSpace}
            tabs={tabViews}
            activeTabId={activeTabId}
            sidebarOpen={sidebarOpen}
            onToggleSidebar={() => setSidebarOpen((o) => !o)}
            onSelect={(id) => activeSpace && setActiveTabBySpace((m) => ({ ...m, [activeSpace.id]: id }))}
            onClose={closeTab}
            onNew={() => activeSpace && openTab(activeSpace.id)}
            onHistory={() => setHistoryOpen(true)}
            onPlugins={() => setPluginsOpen(true)}
            pluginButtons={pluginButtons}
            onPluginButton={(button, rect) => {
              if (button.kind === "panel") {
                void api.pluginPanelToggle(button.pluginId, button.id, rect.right, rect.bottom).catch((e) => console.error(e));
                return;
              }
              setRightSidebar((open) => (open === button.key ? null : button.key));
              setOpenedSidebars((all) => (all.includes(button.key) ? all : [...all, button.key]));
            }}
          />
          <ResizablePanelGroup
            orientation="horizontal"
            elementRef={panelGroup}
            onLayoutChanged={() => setWidths(liveWidths.current)}
            className="min-h-0 flex-1"
          >
            <ResizablePanel
              id="left"
              panelRef={leftPanel}
              collapsible
              collapsedSize={0}
              minSize={200}
              maxSize={420}
              defaultSize={248}
              groupResizeBehavior="preserve-pixel-size"
              className="overflow-hidden"
              onResize={(size) => {
                if (size.inPixels < 1) setSidebarOpen(false);
                else liveWidths.current = { ...liveWidths.current, left: Math.round(size.inPixels) };
              }}
            >
              <Sidebar
                open={sidebarOpen}
                spaces={spaces}
                git={git}
                onRefreshGit={() => api.gitRefresh().then(setGit)}
                agents={agents}
                activeSpaceId={activeSpaceId}
                focusedPaneId={focusedPaneId}
                onSelectSpace={selectSpace}
                onSelectAgent={selectAgent}
                onAddSpace={(path) => void addSpace(path)}
                onRemoveSpace={(id) => void removeSpace(id)}
              />
            </ResizablePanel>
            <SideHandle disabled={!sidebarOpen} />
            <ResizablePanel id="main" minSize={320}>
              <main className="relative h-full min-h-0 min-w-0">
                {tabs.map((tab) => (
                  <PaneGrid
                    key={tab.id}
                    layout={tab.layout}
                    visible={tab.id === activeTabId}
                    focusedPane={tab.focusedPane}
                    zoomedPane={tab.zoomedPane}
                    labels={labels}
                    initialInputs={initialInputs}
                    badges={paneBadges}
                    onBadgeAction={onBadgeAction}
                    actions={actions}
                  />
                ))}
                {!activeSpace && (
                  <Empty className="h-full">
                    <EmptyHeader>
                      <EmptyTitle>No project open</EmptyTitle>
                      <EmptyDescription>Add a project with the + next to Projects to open a terminal in it.</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                )}
              </main>
            </ResizablePanel>
            <SideHandle disabled={rightRef === null} />
            <ResizablePanel
              id="right"
              panelRef={rightPanel}
              collapsible
              collapsedSize={0}
              minSize={280}
              maxSize={640}
              defaultSize={0}
              groupResizeBehavior="preserve-pixel-size"
              className="overflow-hidden"
              onResize={(size) => {
                if (size.inPixels < 1) setRightSidebar(null);
                else liveWidths.current = { ...liveWidths.current, right: Math.round(size.inPixels) };
              }}
            >
              <RightSidebar
                open={rightRef}
                mounted={openedSidebars.flatMap((key) => sidebarRef(key) ?? [])}
                host={pluginHost.current}
                onSelect={setRightSidebar}
                onCloseTab={(id) => {
                  const rest = openedSidebars.filter((k) => k !== id);
                  setOpenedSidebars(rest);
                  if (rightSidebar === id) setRightSidebar(rest.at(-1) ?? null);
                }}
                onClose={() => setRightSidebar(null)}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </div>
        <DiffViewer diff={diff} onClose={() => setDiff(null)} />
        <PluginManager
          open={pluginsOpen}
          onOpenChange={setPluginsOpen}
          plugins={plugins}
          dropped={droppedPlugin}
          onDroppedHandled={() => setDroppedPlugin(null)}
          onChanged={(list, restart) => {
            setPlugins(list);
            pluginHost.current?.sync(list, restart);
          }}
        />
        <HistorySheet
          space={activeSpace}
          open={historyOpen}
          onOpenChange={setHistoryOpen}
          liveSessionIds={liveSessionIds}
          onResume={(id) => {
            setHistoryOpen(false);
            if (activeSpace && isSessionId(id)) openTab(activeSpace.id, resumeCommand(id), id);
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
