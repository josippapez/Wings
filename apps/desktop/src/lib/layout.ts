/** A tab's pane arrangement: a binary tree of splits with terminal panes at the leaves. */
export type LayoutNode =
  | { kind: "pane"; id: string }
  | { kind: "split"; id: string; dir: "row" | "column"; ratio: number; a: LayoutNode; b: LayoutNode };

export const pane = (id: string): LayoutNode => ({ kind: "pane", id });

export function paneIds(node: LayoutNode): string[] {
  return node.kind === "pane" ? [node.id] : [...paneIds(node.a), ...paneIds(node.b)];
}

/** Puts `newId` beside `target`: to the right for "row", below for "column". */
export function split(node: LayoutNode, target: string, dir: "row" | "column", newId: string): LayoutNode {
  if (node.kind === "pane") {
    if (node.id !== target) return node;
    return { kind: "split", id: crypto.randomUUID(), dir, ratio: 0.5, a: node, b: pane(newId) };
  }
  return { ...node, a: split(node.a, target, dir, newId), b: split(node.b, target, dir, newId) };
}

/** Removes a pane; its sibling takes the parent's place. Returns null when the last pane goes. */
export function remove(node: LayoutNode, target: string): LayoutNode | null {
  if (node.kind === "pane") return node.id === target ? null : node;
  const a = remove(node.a, target);
  const b = remove(node.b, target);
  if (!a) return b;
  if (!b) return a;
  return { ...node, a, b };
}

export function setRatio(node: LayoutNode, splitId: string, ratio: number): LayoutNode {
  if (node.kind === "pane") return node;
  if (node.id === splitId) return { ...node, ratio };
  return { ...node, a: setRatio(node.a, splitId, ratio), b: setRatio(node.b, splitId, ratio) };
}

/** Same shape with every pane id replaced, e.g. to give restored panes fresh keys. */
export function mapPanes(node: LayoutNode, fn: (id: string) => string): LayoutNode {
  if (node.kind === "pane") return pane(fn(node.id));
  return { ...node, a: mapPanes(node.a, fn), b: mapPanes(node.b, fn) };
}
