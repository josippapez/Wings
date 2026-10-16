// The Default view setting: which layout the Issues tab opens in. Saved by the settings page, read by the tab.

import { setting } from "wings-plugin-sdk/settings";

export type DefaultView = "board" | "rows" | "list";
export const DEFAULT_VIEWS: { value: DefaultView; label: string }[] = [
  { value: "board", label: "Board" },
  { value: "rows", label: "Board by epic" },
  { value: "list", label: "List" },
];

/** The saved view, or the board when nothing valid is saved. Declared once: objects made for one key don't hear each other. */
export const defaultView = setting<DefaultView>("defaultView", { default: "board", check: (saved) => DEFAULT_VIEWS.find((v) => v.value === saved)?.value });

/** How the tab starts out: rows are the board grouped by epic or feature, the list is grouped by status. */
export const startLayout = (view: DefaultView): Layout => ({ layout: view === "list" ? "list" : "board", byParent: view === "rows" });

export type Layout = { layout: "board" | "list"; byParent: boolean };

/** The layout and rows choice last picked in the tab, so a reopened tab starts as it was left. Cleared when the Default view changes. */
export const lastLayout = setting<Layout | null>("lastLayout", {
  default: null,
  check: (saved) => {
    const s = saved as Partial<Layout> | null;
    return s && (s.layout === "board" || s.layout === "list") && typeof s.byParent === "boolean" ? { layout: s.layout, byParent: s.byParent } : undefined;
  },
});
