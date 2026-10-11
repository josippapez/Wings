// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// Dev servers: when a program in a terminal pane listens on a local port, the pane gets a `localhost:<port>`
// badge with buttons to open it in the browser or in a Wings window. The badge goes when the port closes.
// Wings finds the ports (`wings.ports`, one process-tree walk and one `lsof`, about 20 ms), so this only polls.

const POLL_MS = 5_000;
/** A pane's badge keeps up to 4 actions: two for the first port, and a browser button for each of the next two. */
const MAX_ACTION_PORTS = 3;
const MINUTE = 60_000;

/** @type {{ paneId: string }[]} */
let panes = [];
/** paneId → the ports its badge shows now, and its serialized badge to skip identical updates. */
const shown = new Map();
let checking = false;
let lastCheck = 0;

wings.onPanes((list) => {
  panes = list;
  // A new pane or a `cd` shouldn't wait for the next tick, but a burst of changes costs one lookup.
  if (Date.now() - lastCheck > 1_000) void refresh();
});
setInterval(() => {
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

/**
 * Sets the badge of every pane that listens, and clears the others.
 * @param {{ paneId: string, pid: number, name: string, port: number, startedAt: number }[]} ports @param {number} now
 */
async function update(ports, now) {
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
    shown.set(paneId, { ports: list.map((p) => p.port), signature });
    await wings.setBadge(paneId, badge).catch(() => {});
  }
}

/** @param {{ paneId: string, port: number }[]} ports */
function groupByPane(ports) {
  /** @type {Map<string, any[]>} */
  const map = new Map();
  for (const p of ports) map.set(p.paneId, [...(map.get(p.paneId) ?? []), p]);
  for (const list of map.values()) list.sort((a, b) => a.port - b.port);
  return map;
}

/** `3 min`, `2 h 5 min`, `1 d 4 h`. Minutes are the finest it shows, so the badge changes once a minute. */
function uptime(/** @type {number} */ startedAt, /** @type {number} */ now) {
  if (!startedAt) return "";
  const minutes = Math.max(0, Math.floor((now - startedAt) / MINUTE));
  if (minutes < 1) return "just started";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
  return hours % 24 ? `${Math.floor(hours / 24)} d ${hours % 24} h` : `${Math.floor(hours / 24)} d`;
}

const address = (/** @type {number} */ port) => `http://localhost:${port}/`;

/**
 * The badge for one pane: its lowest port as the label, `+N` for the others, a row for each program, and
 * buttons for the first port plus a browser button for the next two.
 * @param {{ pid: number, name: string, port: number, startedAt: number }[]} list @param {number} now
 */
function badgeFor(list, now) {
  const first = list[0];
  const extra = list.length - 1;
  const actions = [
    { id: `browser:${first.port}`, label: "Open in browser", primary: true },
    { id: `wings:${first.port}`, label: "Open in Wings", primary: false },
  ];
  for (const p of list.slice(1, MAX_ACTION_PORTS)) actions.push({ id: `browser:${p.port}`, label: `Open :${p.port} in browser`, primary: false });
  return {
    label: `localhost:${first.port}${extra > 0 ? ` +${extra}` : ""}`,
    tone: "neutral",
    title: list.length === 1 ? "Listening on this computer" : `Listening on ${list.length} ports`,
    rows: list.slice(0, 10).map((p) => {
      const age = uptime(p.startedAt, now);
      return { label: p.name, value: `:${p.port}${age ? ` · up ${age}` : ""}` };
    }),
    actions,
  };
}

wings.onAction(async ({ paneId, actionId }) => {
  const [kind, text] = actionId.split(":");
  const port = Number(text);
  const known = shown.get(paneId)?.ports ?? [];
  // A click can come from a badge a few seconds old: only open what was listening at the last look.
  if (!Number.isInteger(port) || !known.includes(port)) throw new Error("That port isn't open any more.");
  if (kind === "browser") return wings.openUrl(address(port));
  if (kind === "wings") {
    await wings.storage.set("preview", address(port));
    wings.broadcast({ preview: address(port) });
    return wings.openView("preview", { as: "window" });
  }
  throw new Error("Unknown action");
});
