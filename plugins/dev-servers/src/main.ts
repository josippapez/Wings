// Dev servers: when a program in a terminal pane listens on a local port, the pane gets a `localhost:<port>`
// badge with buttons to open it in the browser. The badge goes when the port closes.
// Wings finds the ports (`wings.ports`, one process-tree walk and one `lsof`, about 20 ms), so this only polls.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { address, badgeFor, MAX_ACTION_PORTS, type Port } from "./lib/badge";

const POLL_MS = 15_000;
/** A server started in a pane takes a few seconds to listen: look again this long after the pane changes. */
const SETTLE_MS = 4_000;

let panes: { paneId: string }[] = [];
/** paneId → the ports its badge has buttons for now, and its serialized badge to skip identical updates. */
const shown = new Map<string, { ports: number[]; signature: string }>();
let checking = false;
let lastCheck = 0;
let settle: ReturnType<typeof setTimeout> | undefined;

wings.onPanes((list) => {
  panes = list;
  // A new pane or a `cd` shouldn't wait for the next tick, but a burst of changes costs one lookup.
  if (Date.now() - lastCheck > 1_000) void refresh();
  clearTimeout(settle);
  settle = setTimeout(() => void refresh(), SETTLE_MS);
});
// Paused while Wings is in the background: lsof once every 15 s for a badge nobody is looking at.
pollWhileFocused(() => {
  if (panes.length > 0) void refresh();
}, POLL_MS);

async function refresh() {
  if (checking) return;
  checking = true;
  lastCheck = Date.now();
  try {
    await update(await wings.ports(), Date.now());
  } catch {
    // Wings couldn't read ports this time (no lsof): keep what's shown and try again on the next tick.
  } finally {
    checking = false;
  }
}

/** Sets the badge of every pane that listens, and clears the others. */
async function update(ports: Port[], now: number) {
  const open = new Set(panes.map((p) => p.paneId));
  const byPane = groupByPane(ports.filter((p) => open.has(p.paneId)));
  for (const paneId of [...shown.keys()]) {
    if (byPane.has(paneId)) continue;
    shown.delete(paneId);
    await wings.setBadge(paneId, null).catch(() => {});
  }
  for (const [paneId, list] of byPane) {
    const badge = badgeFor(list, now);
    const signature = JSON.stringify(badge);
    if (shown.get(paneId)?.signature === signature) continue;
    shown.set(paneId, { ports: list.slice(0, MAX_ACTION_PORTS).map((p) => p.port), signature });
    await wings.setBadge(paneId, badge).catch(() => {});
  }
}

function groupByPane(ports: Port[]) {
  const map = new Map<string, Port[]>();
  for (const p of ports) map.set(p.paneId, [...(map.get(p.paneId) ?? []), p]);
  for (const list of map.values()) list.sort((a, b) => a.port - b.port);
  return map;
}

wings.onAction(async ({ paneId, actionId }) => {
  if (actionId === "refresh") return refresh();
  const [kind, text] = actionId.split(":");
  const port = Number(text);
  const known = shown.get(paneId)?.ports ?? [];
  // A click can come from a badge a few seconds old: only open a port that had a button at the last look.
  if (!Number.isInteger(port) || !known.includes(port)) throw new Error("That port isn't open any more.");
  if (kind === "browser") return wings.openUrl(address(port));
  throw new Error("Unknown action");
});
