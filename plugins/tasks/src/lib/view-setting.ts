// The Default view setting: which layout the Issues tab opens in. Saved by the settings page, read by the tab.

export const DEFAULT_VIEW_KEY = "defaultView";

export type DefaultView = "board" | "rows" | "list";
export const DEFAULT_VIEWS: { value: DefaultView; label: string }[] = [
  { value: "board", label: "Board" },
  { value: "rows", label: "Rows, grouped by parent" },
  { value: "list", label: "List" },
];

/** A saved value, or the board when it holds anything else. */
export const defaultViewOf = (saved: unknown): DefaultView => (saved === "rows" || saved === "list" ? saved : "board");

/** How the tab starts out: rows are the board grouped by epic or feature, the list is grouped by status. */
export const startLayout = (view: DefaultView): { layout: "board" | "list"; byParent: boolean } => ({ layout: view === "list" ? "list" : "board", byParent: view === "rows" });
