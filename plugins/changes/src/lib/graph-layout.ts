import type { Commit } from "./log";

/** Lanes past this are cut off; a repo rarely has this many branches side by side. */
export const MAX_LANES = 24;

/**
 * A commit and where the graph draws it. `lines` holds four numbers a line: from lane, to lane, part and colour,
 * where part 0 runs from the row's top into the dot, 1 from the dot to the row's bottom, and 2 straight through.
 */
export type Placed = Commit & { lane: number; color: number; lines: number[] };

/**
 * Gives each commit a lane, top to bottom in topological order. Each lane waits for one hash: a commit takes the
 * lane waiting for it (the others waiting for it end there), and hands it to its first parent; other parents get
 * a free lane unless one already waits for them. Lanes keep their place, so lines only bend at merges and forks.
 */
export function layout(commits: Commit[]) {
  const lanes: (string | null)[] = [];
  const colors: number[] = [];
  let next = 0;
  let widest = 1;
  const free = () => {
    const i = lanes.indexOf(null);
    return i < 0 ? lanes.length : i;
  };
  const rows: Placed[] = commits.map((c) => {
    let lane = lanes.indexOf(c.hash);
    if (lane < 0) {
      // A branch tip: nothing above leads to it, so it starts in a free lane with a colour of its own.
      lane = free();
      colors[lane] = next++;
    }
    const color = colors[lane];
    const lines: number[] = [];
    for (let j = 0; j < lanes.length; j++) {
      if (lanes[j] === c.hash) {
        lines.push(j, lane, 0, colors[j]);
        lanes[j] = null;
      } else if (lanes[j] !== null) lines.push(j, j, 2, colors[j]);
    }
    lanes[lane] = null;
    c.parents.forEach((p, k) => {
      let at = lanes.indexOf(p);
      if (at < 0) {
        at = k === 0 ? lane : free();
        lanes[at] = p;
        colors[at] = k === 0 ? color : next++;
      }
      lines.push(lane, at, 1, colors[at]);
    });
    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
    widest = Math.max(widest, lane + 1, lanes.length);
    return { ...c, lane, color, lines };
  });
  return { rows, lanes: Math.min(widest, MAX_LANES) };
}
