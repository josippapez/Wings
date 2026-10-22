// The colour of each entry's wedge, for the swatch in front of its row in the list.

import type { Entry } from "../lib/messages";
import { layout } from "../lib/sunburst";
import { parentPath } from "../lib/tree";
import { MIN_ANGLE, RINGS } from "./sunburst";

/** The colour of the wedge at a path, or the nearest one above it that the map draws, or null when it draws none. */
export function swatchesOf(tree: Entry, focus: string, hidden: number | null): (path: string) => string | null {
  const { byPath } = layout(tree, focus, { rings: RINGS, hidden, minAngle: MIN_ANGLE });
  return (path) => {
    for (let p: string | null = path; p && p.length >= focus.length; p = parentPath(p)) {
      const segment = byPath.get(p);
      if (segment) return segment.color;
    }
    return null;
  };
}
