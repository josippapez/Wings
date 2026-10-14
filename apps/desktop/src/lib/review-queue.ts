/** Plugins waiting for you to review their access, shown one at a time, first first. */

/** Adds `items` to the end; one already waiting is updated in its place (a newer version asks for its own access). */
export function enqueue<T extends { id: string }>(queue: T[], ...items: T[]): T[] {
  const next = [...queue];
  for (const item of items) {
    const at = next.findIndex((q) => q.id === item.id);
    if (at >= 0) next[at] = item;
    else next.push(item);
  }
  return next;
}

/** The queue after the first one was turned on or put off. */
export const advance = <T>(queue: T[]): T[] => queue.slice(1);
