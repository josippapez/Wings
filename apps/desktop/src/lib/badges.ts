/**
 * Badge and sidebar label updates that keep the same object when nothing changed, so React skips the render.
 * Plugins resend the badge a pane already has on every poll, and each new object re-rendered all of App.
 */
const same = (a: unknown, b: unknown) => a === b || JSON.stringify(a) === JSON.stringify(b);

export function withBadge<B>(all: Record<string, Record<string, B>>, paneId: string, pluginId: string, badge: B | null) {
  const current = all[paneId]?.[pluginId];
  if (badge ? same(current, badge) : current === undefined) return all;
  const { [pluginId]: _old, ...others } = all[paneId] ?? {};
  return { ...all, [paneId]: badge ? { ...others, [pluginId]: badge } : others };
}

export function withLabel<L>(all: Record<string, L>, key: string, label: L | null) {
  if (label ? same(all[key], label) : !(key in all)) return all;
  const { [key]: _old, ...rest } = all;
  return label ? { ...rest, [key]: label } : rest;
}
