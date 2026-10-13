import { ChevronRightIcon } from "lucide-react";
import { memo, useMemo } from "react";

import { toneClass } from "@/components/pane-badge";
import { PluginGlyph } from "@/components/plugin-glyph";
import type { PluginView } from "@/lib/api";
import { collapseKey, sectionGroups, type ProjectSectionRow } from "@/lib/project-sections";
import { cn } from "@/lib/utils";

export type OpenSectionRow = (projectId: string, pluginId: string, open: ProjectSectionRow["open"]) => void;

/**
 * What plugins list under a project: one collapsible section each, with a row per item. Memoized per project, so a pane-info
 * tick, which re-renders the sidebar, doesn't touch a project whose sections, plugins and collapsed list are the same.
 */
export const ProjectSections = memo(function ProjectSections({
  projectId,
  sections,
  plugins,
  collapsed,
  onToggle,
  onOpen,
}: {
  projectId: string;
  sections: Record<string, ProjectSectionRow[]> | undefined;
  plugins: PluginView[];
  /** Every collapsed section, as `collapseKey` makes them. */
  collapsed: string[];
  onToggle: (projectId: string, key: string) => void;
  onOpen: OpenSectionRow;
}) {
  const groups = useMemo(() => sectionGroups(sections, plugins), [sections, plugins]);
  if (!groups.length) return null;
  return (
    <div className="flex flex-col gap-px pt-px pb-1 pl-6">
      {groups.map((group) => {
        const open = !collapsed.includes(collapseKey(projectId, group.key));
        const listId = `section-${projectId}-${group.key}`.replace(/[^a-zA-Z0-9_-]/g, "_");
        return (
          <section key={group.key} aria-label={group.title}>
            <button
              type="button"
              aria-expanded={open}
              aria-controls={listId}
              onClick={() => onToggle(projectId, group.key)}
              className="flex h-6 w-full items-center gap-1.5 rounded-md px-1.5 text-left text-[11px] font-medium text-muted-foreground outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <ChevronRightIcon className={cn("size-3 shrink-0 transition-transform duration-150 motion-reduce:transition-none", open && "rotate-90")} aria-hidden />
              <PluginGlyph name={group.icon} svg={group.iconSvg} className="size-3.5 shrink-0" />
              <span className="min-w-0 flex-1 truncate">{group.title}</span>
              <span className="tabular-nums">{group.rows.length}</span>
            </button>
            {open && (
              <ul id={listId} className="flex flex-col gap-px">
                {group.rows.map((row) => (
                  <li key={row.id}>
                    <button
                      type="button"
                      onClick={() => onOpen(projectId, group.pluginId, row.open)}
                      title={row.detail ? `${row.label}, ${row.detail}` : row.label}
                      className="flex w-full items-center gap-2 rounded-md py-1 pr-1.5 pl-6 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring/50"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[12px] font-medium">{row.label}</span>
                        {row.detail && <span className="block truncate text-[11px] leading-4 text-muted-foreground">{row.detail}</span>}
                      </span>
                      {row.badge && (
                        <span className={cn("max-w-[40%] shrink-0 truncate rounded-full px-1.5 py-px text-[10px] leading-4 ring-1", toneClass[row.badge.tone].pill)}>{row.badge.text}</span>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
});
