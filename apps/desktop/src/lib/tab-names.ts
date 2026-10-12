export type TabTitleInput = {
  id: string;
  /** The name you gave the tab; it is shown as typed. */
  name?: string;
  /** The automatic title, like the shell or agent name. */
  title: string;
  /** What the automatic title is about: a folder like `~/Desktop/wings`, or a project name. */
  detail: string;
};

const segments = (detail: string) => detail.split(/[\\/]/).filter(Boolean);

/** The shortest tail of path segments that tells each of these paths apart, for as many as can be told apart. */
function tails(details: string[]): Map<string, string> {
  const distinct = [...new Set(details)];
  const parts = distinct.map(segments);
  const deepest = Math.max(1, ...parts.map((p) => p.length));
  for (let k = 1; k <= deepest; k++) {
    const cut = parts.map((p) => p.slice(-k).join("/"));
    if (new Set(cut).size === distinct.length) return new Map(distinct.map((d, i) => [d, cut[i]]));
  }
  return new Map(distinct.map((d, i) => [d, parts[i].join("/")]));
}

/**
 * What each tab shows. A name you gave stays as typed. Tabs left with the same automatic title get the smallest
 * addition that tells them apart: the shortest folder tail that differs (`zsh · wings`), and for tabs that share
 * the folder too a number from the second on (`zsh · wings 2`). The first of identical tabs stays plain.
 */
export function tabTitles(tabs: TabTitleInput[]): Record<string, string> {
  const out: Record<string, string> = {};
  const groups = new Map<string, TabTitleInput[]>();
  for (const t of tabs) {
    if (t.name) out[t.id] = t.name;
    else groups.set(t.title, [...(groups.get(t.title) ?? []), t]);
  }
  for (const [title, group] of groups) {
    if (group.length === 1) {
      out[group[0].id] = title;
      continue;
    }
    const details = group.map((t) => t.detail);
    const tail = new Set(details).size > 1 ? tails(details) : null;
    const seen = new Map<string, number>();
    for (const t of group) {
      const base = tail?.get(t.detail);
      const n = (seen.get(t.detail) ?? 0) + 1;
      seen.set(t.detail, n);
      const text = base ? `${title} · ${base}` : title;
      out[t.id] = n > 1 ? `${text} ${n}` : text;
    }
  }
  return out;
}
