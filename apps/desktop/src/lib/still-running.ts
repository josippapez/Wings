import type { LivePane } from "./api.ts";
import { paneIds, type LayoutNode } from "./layout.ts";

/**
 * Saved panes whose shell still runs in the same project, as saved key to pane id: after a webview reload, and after
 * a restart while the terminal host kept them, since the host gives each pane the id saved for it.
 */
export function stillRunning(saved: { tabs: { spaceId: string; layout: LayoutNode }[]; paneIds?: Record<string, string> } | null, live: LivePane[], spaces: { id: string }[]) {
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
