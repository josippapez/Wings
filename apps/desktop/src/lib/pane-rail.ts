/** One line of a pane rail's list. */
export type RailItem = { text: string; status: "pending" | "in_progress" | "done" };
/** What a plugin floats over a pane's terminal: `wings.setPaneRail`. `open` is the id of one of the plugin's sidebars or views. */
export type PaneRail = { title: string; items: RailItem[]; open?: string };

export const RAIL_TITLE_MAX = 40;
export const RAIL_TEXT_MAX = 200;
export const RAIL_MAX_ITEMS = 30;
const statuses: unknown[] = ["pending", "in_progress", "done"] satisfies RailItem["status"][];

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

/**
 * Plugin data is untrusted. A rail that breaks a limit is refused with the reason, like a pane status, so the
 * plugin's author sees it. `pages` are the plugin's sidebar and view ids.
 */
export function cleanPaneRail(raw: unknown, pages: string[]): PaneRail {
  if (!isObject(raw)) throw new Error("setPaneRail: rail must be an object or null");
  if (typeof raw.title !== "string" || !raw.title) throw new Error("setPaneRail: title must be a non-empty string");
  if (raw.title.length > RAIL_TITLE_MAX) throw new Error(`setPaneRail: title is over ${RAIL_TITLE_MAX} characters`);
  if (!Array.isArray(raw.items)) throw new Error("setPaneRail: items must be an array");
  if (raw.items.length > RAIL_MAX_ITEMS) throw new Error(`setPaneRail: at most ${RAIL_MAX_ITEMS} items`);
  const items = raw.items.map((item, i): RailItem => {
    if (!isObject(item)) throw new Error(`setPaneRail: items[${i}] must be an object`);
    if (typeof item.text !== "string" || !item.text) throw new Error(`setPaneRail: items[${i}].text must be a non-empty string`);
    if (item.text.length > RAIL_TEXT_MAX) throw new Error(`setPaneRail: items[${i}].text is over ${RAIL_TEXT_MAX} characters`);
    if (!statuses.includes(item.status)) throw new Error(`setPaneRail: items[${i}].status must be one of ${statuses.join(", ")}`);
    return { text: item.text, status: item.status as RailItem["status"] };
  });
  let open: string | undefined;
  if (raw.open !== undefined && raw.open !== null) {
    if (typeof raw.open !== "string" || !pages.includes(raw.open)) {
      throw new Error(`setPaneRail: open must be the id of one of the plugin's sidebars or views${pages.length ? ` (${pages.join(", ")})` : ""}`);
    }
    open = raw.open;
  }
  return { title: raw.title, items, ...(open ? { open } : {}) };
}

export type Corner = "top-left" | "top-right" | "bottom-left" | "bottom-right";
export const CORNERS: Corner[] = ["top-right", "top-left", "bottom-right", "bottom-left"];

/** The corner of a `width` by `height` area that a card whose centre is at (`x`, `y`) is nearest to. */
export function nearestCorner(x: number, y: number, width: number, height: number): Corner {
  return `${y < height / 2 ? "top" : "bottom"}-${x < width / 2 ? "left" : "right"}`;
}

/** `done/total`, for the minimised pill. */
export function railProgress(rail: PaneRail) {
  return `${rail.items.filter((i) => i.status === "done").length}/${rail.items.length}`;
}
