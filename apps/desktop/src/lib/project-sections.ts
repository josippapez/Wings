import type { PluginProjectSection, PluginView } from "./api.ts";
import { boundedJson, VIEW_ARGS_MAX } from "./plugin-pages.ts";
import type { BadgeIcon, Tone } from "./plugins.ts";

/** One line of a plugin's section under a project in the sidebar. Clicking it opens `open.view` as a tab in that project. */
export type ProjectSectionRow = {
  id: string;
  label: string;
  detail?: string;
  badge?: { text: string; tone: Tone };
  /** Drawn in the badge's tone in place of the plain dot. */
  icon?: SectionIcon;
  /** The id of another row in the section that this one sits under, indented. A parent that isn't listed leaves the row at the top. */
  parent?: string;
  open: { view: string; args?: unknown };
};

export const SECTION_ROWS_MAX = 50;

const tones: Record<Tone, true> = { neutral: true, info: true, success: true, warning: true, danger: true, merged: true };
/** A row's icon: a pull request's, or a work status drawn like Linear's (an empty circle to a checked one). */
export type SectionIcon = BadgeIcon | "todo" | "in-progress" | "in-review" | "done" | "canceled";
const icons: Record<SectionIcon, true> = {
  "pr-open": true, "pr-merged": true, "pr-closed": true, "pr-draft": true,
  todo: true, "in-progress": true, "in-review": true, done: true, canceled: true,
};

const text = (v: unknown, max: number) => (typeof v === "string" && v ? v.slice(0, max) : undefined);

/**
 * Plugin data is untrusted: keeps up to 50 rows with known fields and bounded text. A row with no usable id or label is
 * left out, and so is a second row with an id already seen. A `parent` is kept when it is a string of 1 to 64 characters that
 * isn't the row's own id; `nestRows` deals with one that names no row. Throws when `rows` isn't a list, or a row opens a view
 * that isn't in `tabViews` (the plugin's views that open as a tab) or its args are over 16 KB, as `openView` would.
 */
export function cleanSectionRows(rows: unknown, tabViews: string[]): ProjectSectionRow[] {
  if (!Array.isArray(rows)) throw new Error("setProjectSection rows must be a list, or null to remove the section");
  const seen = new Set<string>();
  const clean: ProjectSectionRow[] = [];
  for (const raw of rows.slice(0, SECTION_ROWS_MAX)) {
    if (!raw || typeof raw !== "object") continue;
    const r = raw as Record<string, unknown>;
    const id = typeof r.id === "string" && r.id.length <= 64 ? r.id : undefined;
    const label = text(r.label, 120);
    if (!id || !label || seen.has(id)) continue;
    const parent = typeof r.parent === "string" && r.parent.length <= 64 && r.parent !== id ? r.parent : undefined;
    const open = r.open && typeof r.open === "object" ? (r.open as Record<string, unknown>) : {};
    if (typeof open.view !== "string" || !tabViews.includes(open.view)) {
      throw new Error(`Row ${id} opens ${String(open.view)}, which isn't one of the plugin's views that open as a tab`);
    }
    const hasArgs = open.args !== undefined && open.args !== null;
    const b = r.badge && typeof r.badge === "object" ? (r.badge as Record<string, unknown>) : null;
    const badgeText = text(b?.text, 24);
    seen.add(id);
    clean.push({
      id,
      label,
      ...(text(r.detail, 120) ? { detail: text(r.detail, 120) } : {}),
      ...(badgeText ? { badge: { text: badgeText, tone: Object.hasOwn(tones, String(b?.tone)) ? (b?.tone as Tone) : "neutral" } } : {}),
      ...(Object.hasOwn(icons, String(r.icon)) ? { icon: r.icon as SectionIcon } : {}),
      ...(parent ? { parent } : {}),
      open: { view: open.view, ...(hasArgs ? { args: JSON.parse(boundedJson(open.args, VIEW_ARGS_MAX, "Row args")) } : {}) },
    });
  }
  return clean;
}

/** Each project's sections: by project id, then `pluginId:sectionId`. Updates leave the projects they don't touch as they were. */
export type ProjectSections = Record<string, Record<string, ProjectSectionRow[]>>;

export const sectionId = (pluginId: string, section: string) => `${pluginId}:${section}`;

/** Sets or, with no rows, removes one section. Returns `all` itself when nothing changed, so React skips the render. */
export function withSection(all: ProjectSections, projectId: string, pluginId: string, section: string, rows: ProjectSectionRow[] | null): ProjectSections {
  const key = sectionId(pluginId, section);
  const project = all[projectId] ?? {};
  if (!rows?.length) {
    if (!project[key]) return all;
    const { [key]: _gone, ...rest } = project;
    if (Object.keys(rest).length) return { ...all, [projectId]: rest };
    const { [projectId]: _empty, ...others } = all;
    return others;
  }
  if (JSON.stringify(project[key]) === JSON.stringify(rows)) return all;
  return { ...all, [projectId]: { ...project, [key]: rows } };
}

/** Drops everything one plugin showed, as when it's turned off. */
export function withoutPlugin(all: ProjectSections, pluginId: string): ProjectSections {
  const prefix = `${pluginId}:`;
  let next = all;
  for (const [projectId, project] of Object.entries(all)) {
    for (const key of Object.keys(project)) if (key.startsWith(prefix)) next = withSection(next, projectId, pluginId, key.slice(prefix.length), null);
  }
  return next;
}

/** A section as the sidebar draws it. `key` is its identity in the project, and in `collapsed`. */
export type SectionGroup = { key: string; pluginId: string; sectionId: string; title: string; icon: string; iconSvg: string | null; all?: string; sorts?: PluginProjectSection["sorts"]; rows: ProjectSectionRow[] };

/** A project's non-empty sections of plugins that are on, in plugin order and each plugin's declared order. */
export function sectionGroups(project: Record<string, ProjectSectionRow[]> | undefined, plugins: PluginView[]): SectionGroup[] {
  if (!project) return [];
  return plugins.flatMap((p) =>
    (p.contributes.projectSections ?? []).flatMap((s) => {
      const rows = project[sectionId(p.id, s.id)];
      return rows?.length ? [{ key: sectionId(p.id, s.id), pluginId: p.id, sectionId: s.id, title: s.title, icon: s.icon, iconSvg: p.iconSvg, all: s.all || undefined, sorts: s.sorts?.length ? s.sorts : undefined, rows }] : [];
    }),
  );
}

/** The collapse state key of a section in a project. It's only compared, never taken apart. */
export const collapseKey = (projectId: string, key: string) => `${projectId}\n${key}`;

/** Adds or removes `key`, so a section collapsed twice is open again. */
export const toggleCollapsed = (collapsed: string[], key: string) => (collapsed.includes(key) ? collapsed.filter((k) => k !== key) : [...collapsed, key]);

/** The collapsed sections from a saved workspace: only the keys of projects that still exist. Anything else in the file is ignored. */
export const restoreCollapsed = (saved: unknown, projectIds: string[]) =>
  Array.isArray(saved) ? saved.filter((k): k is string => typeof k === "string" && projectIds.some((id) => k.startsWith(`${id}\n`))) : [];

/** Drops a removed project's sections. */
export function withoutProject(all: ProjectSections, projectId: string): ProjectSections {
  const { [projectId]: _gone, ...rest } = all;
  return projectId in all ? rest : all;
}

/** Drops a removed project's collapse keys. */
export const withoutProjectCollapsed = (collapsed: string[], projectId: string) => collapsed.filter((k) => !k.startsWith(`${projectId}\n`));

/** Rows a section lists before "Show more". */
export const SECTION_ROWS_SHOWN = 5;

/** A row draws at most this many levels in. Rows nested deeper draw here too, still in tree order. */
export const SECTION_DEPTH_MAX = 3;

/**
 * The rows in the order they draw, each followed by the rows that name it as their `parent` and theirs, siblings in the order
 * the plugin sent them. A row whose parent isn't in `rows` is a top-level row. Rows in or under a loop of parents start at
 * the top instead, in the order sent, so every row appears exactly once.
 */
export function nestRows(rows: ProjectSectionRow[]): { row: ProjectSectionRow; depth: number }[] {
  const ids = new Set(rows.map((r) => r.id));
  const below = new Map<string, ProjectSectionRow[]>();
  const top: ProjectSectionRow[] = [];
  for (const r of rows) {
    if (r.parent && r.parent !== r.id && ids.has(r.parent)) below.set(r.parent, [...(below.get(r.parent) ?? []), r]);
    else top.push(r);
  }
  const out: { row: ProjectSectionRow; depth: number }[] = [];
  const seen = new Set<ProjectSectionRow>();
  const walk = (row: ProjectSectionRow, depth: number) => {
    if (seen.has(row)) return;
    seen.add(row);
    out.push({ row, depth: Math.min(depth, SECTION_DEPTH_MAX) });
    for (const child of below.get(row.id) ?? []) walk(child, depth + 1);
  };
  for (const r of top) walk(r, 0);
  // What no top-level row reaches is in or under a loop of parents.
  for (const r of rows) walk(r, 0);
  return out;
}

/**
 * A row's short reference and what's left of its detail: `#6399 · needs your review` is `#6399` and `needs your review`,
 * `Defect #185638` is `#185638` and `Defect`. The reference is the first `#123` or `!123` word, wherever it is.
 */
export function rowRef(detail: string | undefined): { ref?: string; rest?: string } {
  if (!detail) return {};
  const m = /(?<=^|\s)[#!]\d+(?=\s|$)/.exec(detail);
  if (!m) return { rest: detail };
  const rest = `${detail.slice(0, m.index)}${detail.slice(m.index + m[0].length)}`.split(" · ").map((p) => p.trim()).filter(Boolean).join(" · ");
  return { ref: m[0], ...(rest ? { rest } : {}) };
}

/** What a screen reader says for a row, and the tooltip's text: `label, status, detail`. */
export const rowName = (row: ProjectSectionRow) => [row.label, row.badge?.text, row.detail].filter(Boolean).join(", ");

/** A row's title without its leading `[tag]` groups; the original when nothing else is left. Brackets later in the title stay. */
export const rowTitle = (label: string) => label.replace(/^(?:\s*\[[^\]]*\])+\s*/, "") || label;
