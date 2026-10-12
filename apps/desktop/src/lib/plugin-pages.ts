import type { PluginView } from "@/lib/api";

/** The Settings section of a plugin's own settings page. Plugin ids can't hold `:`. */
export const pluginSettingsSection = (pluginId: string) => `plugin:${pluginId}` as const;

/** Plugins that are on and have a settings page, by name, as Settings lists them under Plugin settings. */
export const settingsPlugins = (plugins: PluginView[]) =>
  plugins.filter((p) => p.enabled && p.approved && p.contributes.settings).sort((a, b) => a.name.localeCompare(b.name));

/** The section to show: a plugin's settings page only while that plugin is on and has one, else Plugins. */
export function shownSection<S extends string>(section: S, plugins: PluginView[]): S | "plugins" {
  if (!section.startsWith("plugin:")) return section;
  return settingsPlugins(plugins).some((p) => pluginSettingsSection(p.id) === section) ? section : "plugins";
}

/** A plugin view open as a tab in a project's tab strip: its page fills the pane area. */
export type ViewTab = { id: string; spaceId: string; view: { pluginId: string; viewId: string } };
/** A view tab as saved, with its place among all the saved tabs. */
export type SavedViewTab = ViewTab & { at: number };

export const isViewTab = <T extends object>(tab: T | ViewTab): tab is ViewTab => "view" in tab;

/** Adds the view's tab to the project, or finds the one already there: one tab per view per project. */
export function openViewTab<T extends { id: string; spaceId: string }>(tabs: (T | ViewTab)[], spaceId: string, view: ViewTab["view"], newId: string) {
  const open = tabs.find((t) => isViewTab(t) && t.spaceId === spaceId && t.view.pluginId === view.pluginId && t.view.viewId === view.viewId);
  if (open) return { tabs, id: open.id };
  return { tabs: [...tabs, { id: newId, spaceId, view }], id: newId };
}

/** The ids of view tabs whose plugin is off or gone, or whose view no longer opens as a tab. */
export function staleViewTabs<T extends object>(tabs: (T | ViewTab)[], plugins: PluginView[]) {
  return tabs.flatMap((t) => {
    if (!isViewTab(t)) return [];
    const plugin = plugins.find((p) => p.id === t.view.pluginId && p.enabled && p.approved);
    // Only `views` can open as a tab: sidebars open in the sidebar only.
    const ok = plugin?.contributes.views.some((v) => v.id === t.view.viewId && v.opens.includes("tab"));
    return ok ? [] : [t.id];
  });
}

/**
 * Splits the tabs for saving: terminal tabs stay in `tabs`, and view tabs go in `viewTabs` with their place, so an
 * older Wings, which reads every saved tab as panes, still loads the rest.
 */
export function saveTabs<T extends object>(tabs: (T | ViewTab)[]) {
  const panes: T[] = [];
  const viewTabs: SavedViewTab[] = [];
  tabs.forEach((t, at) => (isViewTab(t) ? viewTabs.push({ ...t, at }) : panes.push(t)));
  return { tabs: panes, viewTabs };
}

/** Puts saved view tabs back among the restored terminal tabs, each at its old place or at the end. */
export function restoreTabs<T extends object>(panes: T[], viewTabs: SavedViewTab[] = []) {
  const tabs: (T | ViewTab)[] = [...panes];
  for (const { at, ...tab } of [...viewTabs].sort((a, b) => a.at - b.at)) tabs.splice(Math.min(at, tabs.length), 0, tab);
  return tabs;
}
