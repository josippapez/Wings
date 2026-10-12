/** Moves one item to a new index; same list back when nothing changes. */
export function reorder<T>(list: T[], from: number, to: number): T[] {
  const end = Math.max(0, Math.min(list.length - 1, to));
  if (from < 0 || from >= list.length || from === end) return list;
  const next = [...list];
  next.splice(end, 0, ...next.splice(from, 1));
  return next;
}

/**
 * Moves a tab to `toIndex` among its project's tabs. All projects' tabs share one list, so the other projects' tabs
 * keep their places and only this project's slots are refilled in the new order.
 */
export function moveTab<T extends { id: string; spaceId: string }>(tabs: T[], id: string, toIndex: number): T[] {
  const tab = tabs.find((t) => t.id === id);
  if (!tab) return tabs;
  const mine = tabs.filter((t) => t.spaceId === tab.spaceId);
  const moved = reorder(mine, mine.indexOf(tab), toIndex);
  if (moved === mine) return tabs;
  let at = 0;
  return tabs.map((t) => (t.spaceId === tab.spaceId ? moved[at++] : t));
}

/** `items` in the order of `order` (a saved list of ids); ones it doesn't list keep their order at the end. */
export function sortByOrder<T extends { id: string }>(items: T[], order: string[] | undefined): T[] {
  if (!order?.length) return items;
  const rank = new Map(order.map((id, i) => [id, i]));
  const at = (t: T) => rank.get(t.id) ?? order.length;
  return [...items].sort((a, b) => at(a) - at(b));
}
