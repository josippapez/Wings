import { changes, type Changes } from "./changes";
import { rootOf } from "./git";
import { plural } from "./format";
import { panes } from "./panes";

const lines = (c: Changes) => `+${c.added} −${c.removed}`;

function badgeFor(c: Changes): WingsBadge {
  const kinds = { M: "modified", A: "added", D: "deleted", R: "renamed", "?": "new" };
  const by: Record<string, number> = {};
  for (const f of c.files) by[f.status] = (by[f.status] ?? 0) + 1;
  const summary = Object.entries(by)
    .map(([s, n]) => `${n} ${kinds[s as keyof typeof kinds] ?? s}`)
    .join(", ");
  return {
    label: c.added || c.removed ? lines(c) : "changed",
    tone: "neutral",
    counts: [{ icon: "changed", value: c.files.length }],
    title: plural(c.files.length, "changed file"),
    subtitle: `${c.name} · ${c.branch}`,
    rows: [
      { label: "Files", value: summary },
      { label: "Lines", value: c.partial ? `${lines(c)}, not counting every new file` : lines(c) },
    ],
    actions: [
      { id: "open", label: "Open changes", primary: true },
      { id: "branch", label: "Changes on this branch" },
      { id: "sidebar", label: "Show in sidebar" },
      { id: "refresh", label: "Refresh" },
    ],
  };
}

/** Looks up each repo once, then badges every pane in it. */
export async function refreshBadges(options?: { fresh?: boolean }) {
  const list = panes;
  const byPane = await Promise.all(list.map(async (p) => [p, await rootOf(p.cwd).catch(() => null)] as const));
  const results = new Map<string, Changes | null>();
  for (const root of new Set(byPane.map(([, r]) => r).filter((r): r is string => Boolean(r)))) {
    results.set(root, await changes(root, "uncommitted", options).catch((error) => (console.error(error), null)));
  }
  for (const [pane, root] of byPane) {
    const c = root ? results.get(root) : null;
    void wings.setBadge(pane.paneId, c && c.files.length ? badgeFor(c) : null).catch(() => {});
  }
}
