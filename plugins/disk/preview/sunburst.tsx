// The disk map on its own, filling the tab: the startup disk with its hidden space, a home folder, a folder deep inside it,
// with some of it in the Collector, and with the pointer on a wedge and on the centre.
import { useEffect, useRef } from "react";
import { geometryOf, layout } from "../src/lib/sunburst";
import { MIN_ANGLE, RINGS, Sunburst, type SunburstProps } from "../src/ui/sunburst";
import { homePath, root, tree } from "./_tree";

export const page = { place: "tab", fill: true };

const noop = () => {};
const base: SunburstProps = { tree, focus: root, hidden: null, collected: new Set(), selected: null, onPoint: noop, onClick: noop, onUp: noop };

/** What the pointer is on: the centre, the hidden space, a path in the tree, or the smaller objects of a folder. */
type Pointer = "center" | "hidden" | { path: string } | { smaller: string };

/** The map, with the pointer moved onto `at` once it has drawn, as a hand would. */
function Disk({ at, ...props }: Partial<SunburstProps> & { at?: Pointer }) {
  const all = { ...base, ...props };
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!at) return;
    const canvas = host.current!.querySelector("canvas")!;
    const box = canvas.getBoundingClientRect();
    const map = layout(all.tree, all.focus, { rings: RINGS, hidden: all.focus === all.tree.name ? all.hidden : null, minAngle: MIN_ANGLE });
    const g = geometryOf(box.width, box.height, map.rings);
    let x = g.cx;
    let y = g.cy;
    if (at !== "center") {
      const s =
        at === "hidden" ? map.segments.find((m) => m.target.kind === "hidden") : "path" in at ? map.byPath.get(at.path) : map.segments.find((m) => m.target.kind === "smaller" && m.target.folder === at.smaller);
      if (!s) throw new Error(`No wedge for ${JSON.stringify(at)}`);
      const angle = (s.a0 + s.a1) / 2;
      const radius = g.inner + (s.ring + 0.5) * g.ringWidth;
      x += radius * Math.sin(angle);
      y -= radius * Math.cos(angle);
    }
    canvas.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: box.left + x, clientY: box.top + y, pointerId: 1 }));
  }, []);
  // The tab's frame is a column the page fills.
  return (
    <div ref={host} style={{ flex: 1, minHeight: 0 }}>
      <Sunburst {...all} />
    </div>
  );
}

const developer = `${homePath}/Library/Developer`;

export const states = {
  // The startup disk, with the space the scan couldn't account for as the dark wedge at the end of the first ring.
  root: { element: <Disk hidden={52e9} rootLabel="Macintosh HD" /> },
  // A home folder.
  home: { element: <Disk focus={homePath} /> },
  // A folder deep inside it: fewer rings, so each is wider.
  deep: { element: <Disk focus={developer} /> },
  // Two folders in the Collector, one of them with a lot in it, and a file picked in the list.
  collected: { element: <Disk focus={homePath} collected={new Set([`${homePath}/Library/Developer/Xcode`, `${homePath}/Movies`])} selected={`${homePath}/Downloads/Xcode_27.xip`} /> },
  // The pointer on a folder: the middle says what it is.
  hover: { element: <Disk focus={homePath} at={{ path: developer }} /> },
  // The pointer on a file with a long name.
  file: { element: <Disk focus={homePath} at={{ path: `${homePath}/Downloads/ubuntu-26.04-desktop-arm64.iso` }} /> },
  // The pointer on the smaller objects of a folder, and on the hidden space.
  smaller: { element: <Disk focus={homePath} at={{ smaller: `${homePath}/Library` }} /> },
  hidden: { element: <Disk hidden={52e9} at="hidden" /> },
  // The pointer on the centre of a map that isn't at the top: a click goes up.
  up: { element: <Disk focus={developer} at="center" /> },
};
