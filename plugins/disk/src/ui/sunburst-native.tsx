// The disk map in the native app: the page lays the wedges out as the canvas does, and the app draws them, lights the one under the
// pointer and tells the page which it is. There is no zoom between folders, and no dragging a wedge to the Collector.

import { useMemo, useState } from "react";
import { Sunburst as Rings } from "wings-plugin-ui/native";

import { layout, type Segment, type Target } from "../lib/sunburst";
import { find } from "../lib/tree";
import { FADED, MIN_ANGLE, RINGS, canGoUp, fadedIn, wordsOf, type SunburstProps } from "./sunburst";

/** An angle to four places, which a wedge of a few pixels doesn't miss. */
const angle = (a: number) => Math.round(a * 1e4) / 1e4;

export function NativeSunburst({ tree, focus, hidden, collected, selected, rootLabel, onPoint, onClick, onUp }: SunburstProps) {
  const map = useMemo(() => layout(tree, focus, { rings: RINGS, hidden: focus === tree.name ? hidden : null, minAngle: MIN_ANGLE }), [tree, focus, hidden]);
  const faded = useMemo(() => fadedIn(map, collected), [map, collected]);
  const wedges = useMemo(() => map.segments.map((s) => ({ from: angle(s.a0), to: angle(s.a1), ring: s.ring, color: s.color, alpha: faded.has(s) ? FADED : 1 })), [map, faded]);
  const focused = useMemo(() => find(tree, focus)?.at(-1) ?? null, [tree, focus]);
  // What the pointer is on: a wedge's index, -2 for the middle, -1 for nothing.
  const [pointed, setPointed] = useState(-1);
  const hover: Segment | "center" | null = pointed === -2 ? "center" : (map.segments[pointed] ?? null);
  const words = wordsOf({ tree, focus, rootLabel }, focused, hover);
  const picked = selected === null || !map.byPath.has(selected) ? -1 : map.segments.indexOf(map.byPath.get(selected)!);

  return (
    <Rings
      rings={Math.max(1, map.rings)}
      wedges={wedges}
      selected={picked >= 0 ? picked : undefined}
      title={words.title}
      detail={words.details[0]}
      up={canGoUp({ tree, focus })}
      onPoint={(index) => {
        setPointed(index);
        const target: Target | null = index === -2 ? { kind: "entry", path: focus } : (map.segments[index]?.target ?? null);
        onPoint(target);
      }}
      onSelect={(index, event) => {
        if (index === -2) onUp();
        else if (map.segments[index]) onClick(map.segments[index].target, event);
      }}
    />
  );
}
