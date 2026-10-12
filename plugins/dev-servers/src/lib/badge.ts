/** A pane's badge has a browser button for each of its first three ports. */
export const MAX_ACTION_PORTS = 3;
const MINUTE = 60_000;

/** One listening port, as `wings.ports()` reports it. */
export type Port = Awaited<ReturnType<Wings["ports"]>>[number];

/** `3 min`, `2 h 5 min`, `1 d 4 h`. Minutes are the finest it shows, so the badge changes once a minute. */
function uptime(startedAt: number, now: number) {
  if (!startedAt) return "";
  const minutes = Math.max(0, Math.floor((now - startedAt) / MINUTE));
  if (minutes < 1) return "just started";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
  return hours % 24 ? `${Math.floor(hours / 24)} d ${hours % 24} h` : `${Math.floor(hours / 24)} d`;
}

export const address = (port: number) => `http://localhost:${port}/`;

/**
 * The badge for one pane: its lowest port as the label, `+N` for the others, a row for each program, and
 * a browser button for each of the first three.
 */
export function badgeFor(list: Port[], now: number): WingsBadge {
  const first = list[0];
  const extra = list.length - 1;
  const actions = [
    { id: `browser:${first.port}`, label: "Open in browser", primary: true },
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
