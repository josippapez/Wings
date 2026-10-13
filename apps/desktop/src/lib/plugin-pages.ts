import type { PluginView } from "@/lib/api";

/** A page the + menu offers: a view that opens as a tab and sets `newTab`. `key` is `pluginId:viewId`. */
export type NewTabEntry = { key: string; pluginId: string; viewId: string; title: string; icon: string; iconSvg: string | null; multiple: boolean };

/** The + menu's plugin pages, in plugin order, from plugins that are on and approved. */
export const newTabEntries = (plugins: PluginView[]): NewTabEntry[] =>
  plugins
    .filter((p) => p.enabled && p.approved)
    .flatMap((p) =>
      p.contributes.views
        .filter((v) => v.newTab && v.opens.includes("tab"))
        .map((v) => ({ key: `${p.id}:${v.id}`, pluginId: p.id, viewId: v.id, title: v.title, icon: v.icon, iconSvg: p.iconSvg, multiple: v.multiple === true })),
    );

/** What the plain + opens: the page chosen in Settings while it's still offered, else a terminal (null). */
export const defaultNewTab = (entries: NewTabEntry[], chosen: string | null) => entries.find((e) => e.key === chosen) ?? null;

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

/** What `wings.setTabState` may save per tab, and what `openView` may pass as `args`, as JSON text. */
export const TAB_STATE_MAX = 64 * 1024;
export const VIEW_ARGS_MAX = 16 * 1024;

/**
 * A plugin view open as a tab in a project's tab strip: its page fills the pane area. `state` is what the page saved with
 * `setTabState`, `args` what `openView` passed, and `rev` counts the times new args reloaded the page (not saved).
 */
export type ViewTab = { id: string; spaceId: string; view: { pluginId: string; viewId: string }; state?: unknown; args?: unknown; rev?: number };
/** A view tab as saved, with its place among all the saved tabs. */
export type SavedViewTab = Omit<ViewTab, "rev"> & { at: number };

/** `value` as JSON text if it is JSON and fits `max` characters; throws a message naming `what` otherwise. */
export function boundedJson(value: unknown, max: number, what: string) {
  const text = JSON.stringify(value);
  if (text === undefined) throw new Error(`${what} must be a JSON value`);
  if (text.length > max) throw new Error(`${what} is limited to ${max / 1024} KB`);
  return text;
}

export const isViewTab = <T extends object>(tab: T | ViewTab): tab is ViewTab => "view" in tab;

/** JSON text with object keys sorted, so equal values compare equal whatever order their keys came in. */
const canonicalJson = (value: unknown) => JSON.stringify(value, (_, v) => (v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) : v));

/**
 * Adds the view's tab to the project, or finds the one already there: one tab per view per project, unless `multiple`, which opens a new one unless a tab of it already has the same `args`.
 * `args` is what the page reads with `viewArgs`. Opening the existing tab with args that differ from its own replaces
 * them, drops its `state` and bumps its `rev`, which reloads the page; without args the tab stays as it is.
 */
export function openViewTab<T extends { id: string; spaceId: string }>(
  tabs: (T | ViewTab)[],
  spaceId: string,
  view: ViewTab["view"],
  newId: string,
  { multiple = false, args }: { multiple?: boolean; args?: unknown } = {},
) {
  const sameView = (t: T | ViewTab): t is ViewTab => isViewTab(t) && t.spaceId === spaceId && t.view.pluginId === view.pluginId && t.view.viewId === view.viewId;
  // A `multiple` view opens a new tab each time, except for args a tab of it already has: that one is shown.
  if (multiple && args !== undefined) {
    const same = tabs.find((t): t is ViewTab => sameView(t) && t.args !== undefined && canonicalJson(t.args) === canonicalJson(args));
    if (same) return { tabs, id: same.id };
  }
  const open = multiple ? undefined : tabs.find(sameView);
  if (open) {
    if (args === undefined || JSON.stringify(open.args) === JSON.stringify(args)) return { tabs, id: open.id };
    // The saved state belonged to the old args, so it goes.
    const { state: _state, ...rest } = open;
    return { tabs: tabs.map((t) => (t === open ? { ...rest, args, rev: (open.rev ?? 0) + 1 } : t)), id: open.id };
  }
  const tab: ViewTab = { id: newId, spaceId, view, ...(args === undefined ? {} : { args }) };
  return { tabs: [...tabs, tab], id: newId };
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

/** A project's Project page open as a tab: one per project. Its content lives in Wings' storage, so nothing else is saved here. */
export type ProjectTab = { id: string; spaceId: string; project: true };
/** A Project page tab as saved, with its place among all the saved tabs. */
export type SavedProjectTab = ProjectTab & { at: number };

export const isProjectTab = <T extends object>(tab: T | ProjectTab): tab is ProjectTab => "project" in tab;

/** Adds the project's Project page tab, or finds the one already there. */
export function openProjectTab<T extends { id: string; spaceId: string }>(tabs: (T | ProjectTab)[], spaceId: string, newId: string) {
  const open = tabs.find((t): t is ProjectTab => isProjectTab(t) && t.spaceId === spaceId);
  if (open) return { tabs, id: open.id };
  const tab: ProjectTab = { id: newId, spaceId, project: true };
  return { tabs: [...tabs, tab], id: newId };
}

/**
 * Splits the tabs for saving: terminal tabs stay in `tabs`, and view tabs and Project page tabs go apart with their
 * place, so an older Wings, which reads every saved tab as panes, still loads the rest.
 */
export function saveTabs<T extends object>(tabs: (T | ViewTab | ProjectTab)[]) {
  const panes: T[] = [];
  const viewTabs: SavedViewTab[] = [];
  const projectTabs: SavedProjectTab[] = [];
  tabs.forEach((t, at) => {
    if (isProjectTab(t)) return void projectTabs.push({ ...t, at });
    if (!isViewTab(t)) return void panes.push(t);
    const { rev: _rev, ...saved } = t;
    viewTabs.push({ ...saved, at });
  });
  return { tabs: panes, viewTabs, projectTabs };
}

/** Puts saved view tabs and Project page tabs back among the restored terminal tabs, each at its old place or at the end. */
export function restoreTabs<T extends object>(panes: T[], viewTabs: SavedViewTab[] = [], projectTabs: SavedProjectTab[] = []) {
  const tabs: (T | ViewTab | ProjectTab)[] = [...panes];
  const others: (SavedViewTab | SavedProjectTab)[] = [...viewTabs, ...projectTabs];
  for (const { at, ...tab } of others.sort((a, b) => a.at - b.at)) tabs.splice(Math.min(at, tabs.length), 0, tab);
  return tabs;
}
