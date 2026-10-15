// Dev servers: when a program in a terminal pane listens on a local port, the pane gets a `localhost:<port>`
// badge with buttons to open it in the browser. The badge goes when the port closes.
// Wings finds the ports (`wings.ports`, one process-tree walk and one `lsof`, about 20 ms), so this only polls.

import { errorMessage, singleFlight } from "wings-plugin-sdk/async";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { address, badgeFor, MAX_ACTION_PORTS, type Port } from "./lib/badge";

const POLL_MS = 15_000;
/** A server started in a pane takes a few seconds to listen: look again this long after the pane changes. */
const SETTLE_MS = 4_000;

let panes: { paneId: string }[] = [];
/** paneId → the ports its badge has buttons for, as of the last look. */
let buttons = new Map<string, number[]>();
let lastCheck = 0;
let settle: ReturnType<typeof setTimeout> | undefined;

/** Looks at the ports. A call while a look is running makes it look once more when it is done. */
const refresh = singleFlight(look);

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

/**
 * Sets the badge of every open pane: the ones that listen get one, the others lose theirs. Wings skips a badge a pane
 * already shows and drops the badges of panes that close, so each look sends them all.
 */
async function look() {
  lastCheck = Date.now();
  const ports = await wings.ports().catch((error) => {
    // Wings couldn't read ports this time (no lsof): keep the badges as they are and look again on the next tick.
    console.warn("[dev-servers] couldn't read ports:", errorMessage(error));
  });
  if (!ports) return;
  const now = Date.now();
  const byPane = groupByPane(ports);
  buttons = new Map();
  await Promise.all(
    panes.map(({ paneId }) => {
      const list = byPane.get(paneId);
      if (list) buttons.set(paneId, list.slice(0, MAX_ACTION_PORTS).map((p) => p.port));
      return wings.setBadge(paneId, list ? badgeFor(list, now) : null).catch((error) => {
        console.warn(`[dev-servers] couldn't set the badge of pane ${paneId}:`, errorMessage(error));
      });
    }),
  );
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
  // A click can come from a badge a few seconds old: only open a port that had a button at the last look.
  if (!Number.isInteger(port) || !buttons.get(paneId)?.includes(port)) throw new Error("That port isn't open any more.");
  if (kind === "browser") return wings.openUrl(address(port));
  throw new Error("Unknown action");
});
