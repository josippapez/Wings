import { paneIds, remove, type LayoutNode } from "./layout.ts";

type PaneTab = { id: string; spaceId: string; layout: LayoutNode; focusedPane: string; zoomedPane: string | null };
type Update<T> = (change: (value: T) => T) => void;

/**
 * Closing panes and tabs. `tabs` is the ref event handlers read, which follows React state only on its next render,
 * so each change goes to the ref too: panes exiting back to back (a terminal host that died) each close from what the
 * one before left. Closing from the tabs before them all put back a tab or a pane already gone, and the UI threw.
 */
export function tabCloser<T extends { id: string; spaceId: string }>(tabs: { current: T[] }, setTabs: Update<T[]>, setActive: Update<Record<string, string>>, dispose: (key: string) => void) {
  const isPaneTab = (t: T): t is T & PaneTab => "layout" in t;
  const change = (update: (ts: T[]) => T[]) => {
    tabs.current = update(tabs.current);
    setTabs(update);
  };

  /** Closes tabs; a project showing one of them shows its last other tab. */
  const closeTabs = (tabIds: string[]) => {
    const gone = tabs.current.filter((t) => tabIds.includes(t.id));
    if (!gone.length) return;
    for (const tab of gone) if (isPaneTab(tab)) paneIds(tab.layout).forEach(dispose);
    change((ts) => ts.filter((t) => !tabIds.includes(t.id)));
    const rest = tabs.current;
    setActive((m) => {
      const next = { ...m };
      for (const tab of gone) {
        if (next[tab.spaceId] !== tab.id) continue;
        const siblings = rest.filter((t) => t.spaceId === tab.spaceId);
        if (siblings.length) next[tab.spaceId] = siblings[siblings.length - 1].id;
        else delete next[tab.spaceId];
      }
      return next;
    });
  };

  /** Closes a pane, and its tab with it when it was the last one there. */
  const closePane = (key: string) => {
    const tab = tabs.current.find((t): t is T & PaneTab => isPaneTab(t) && paneIds(t.layout).includes(key));
    if (!tab) return;
    const layout = remove(tab.layout, key);
    if (!layout) return closeTabs([tab.id]);
    dispose(key);
    change((ts) =>
      ts.map((t) =>
        t.id === tab.id && isPaneTab(t)
          ? { ...t, layout, focusedPane: t.focusedPane === key ? paneIds(layout)[0] : t.focusedPane, zoomedPane: t.zoomedPane === key ? null : t.zoomedPane }
          : t,
      ),
    );
  };

  return { closeTabs, closePane };
}
