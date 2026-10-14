import { ChevronRightIcon, CircleCheckIcon, CircleDotDashedIcon, CircleDotIcon, CircleIcon, CircleXIcon } from "lucide-react";
import { memo, useMemo, useState } from "react";

import { iconFor, toneClass } from "@/components/pane-badge";
import type { PluginView } from "@/lib/api";
import { wantsNewTab } from "@/lib/plugin-pages";
import { collapseKey, rowName, rowRef, rowTitle, sectionGroups, SECTION_ROWS_SHOWN, type ProjectSectionRow, type SectionIcon } from "@/lib/project-sections";
import { cn } from "@/lib/utils";

export type OpenSectionRow = (projectId: string, pluginId: string, open: ProjectSectionRow["open"], newTab?: boolean) => void;

/**
 * What plugins list under a project: one collapsible section each, with a one-line row per item (a dot in the badge's tone,
 * the label, the short reference). The badge text and detail are in the tooltip and the accessible name. Memoized per project,
 * so a pane-info tick, which re-renders the sidebar, doesn't touch a project whose sections, plugins and collapsed list are the same.
 */
/** A row's icon: a pull request's state, or a work status (to do, in progress, in review, done, canceled). */
const rowIcon: Record<SectionIcon, typeof CircleIcon> = {
  ...iconFor,
  todo: CircleIcon,
  "in-progress": CircleDotDashedIcon,
  "in-review": CircleDotIcon,
  done: CircleCheckIcon,
  canceled: CircleXIcon,
};

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
  /** Sections whose full list is showing. Kept for the session, not saved. */
  const [expanded, setExpanded] = useState<string[]>([]);
  if (!groups.length) return null;
  return (
    <div className="group/sections flex flex-col pt-px pb-1 pl-5" data-testid="project-sections">
      {groups.map((group) => {
        const open = !collapsed.includes(collapseKey(projectId, group.key));
        const listId = `section-${projectId}-${group.key}`.replace(/[^a-zA-Z0-9_-]/g, "_");
        const all = expanded.includes(group.key);
        const rows = all ? group.rows : group.rows.slice(0, SECTION_ROWS_SHOWN);
        const hidden = group.rows.length - rows.length;
        return (
          <section key={group.key} aria-label={group.title}>
            <div className="group/head flex items-center">
            <button
              type="button"
              aria-expanded={open}
              aria-controls={listId}
              title={`${group.title}, ${group.rows.length}`}
              aria-label={`${group.title}, ${group.rows.length} ${group.rows.length === 1 ? "item" : "items"}`}
              onClick={() => onToggle(projectId, group.key)}
              className="group/header flex h-6 min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 text-left text-[11px] text-muted-foreground outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <span className="truncate">{group.title}</span>
              <ChevronRightIcon className={cn("size-3 shrink-0 transition-transform duration-150 motion-reduce:transition-none", open ? "hidden rotate-90 group-hover/sections:block group-focus-visible/header:block" : "block")} aria-hidden />
            </button>
            {group.all && (
              <button
                type="button"
                onClick={(e) => onOpen(projectId, group.pluginId, { view: group.all! }, wantsNewTab(e))}
                onAuxClick={(e) => {
                  if (e.button !== 1) return;
                  e.preventDefault();
                  onOpen(projectId, group.pluginId, { view: group.all! }, true);
                }}
                title={`All ${group.title.toLowerCase()}`}
                aria-label={`All ${group.title.toLowerCase()}`}
                className="h-6 shrink-0 rounded-md px-1.5 text-[11px] text-muted-foreground outline-none hover:bg-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                All
              </button>
            )}
            </div>
            {open && (
              <ul id={listId} className="flex flex-col">
                {rows.map((row) => {
                  const { ref } = rowRef(row.detail);
                  const name = rowName(row);
                  const t = row.badge?.tone ?? "neutral";
                  const tone = toneClass[t].text;
                  const filled = t === "success" || t === "danger" || t === "merged";
                  const Icon = row.icon ? rowIcon[row.icon] : null;
                  return (
                    <li key={row.id}>
                      <button
                        type="button"
                        onClick={(e) => onOpen(projectId, group.pluginId, row.open, wantsNewTab(e))}
                        onAuxClick={(e) => {
                          if (e.button !== 1) return;
                          e.preventDefault();
                          onOpen(projectId, group.pluginId, row.open, true);
                        }}
                        title={name}
                        aria-label={name}
                        data-testid="section-row"
                        className="group/row flex h-7 w-full items-center gap-2 rounded-md px-1.5 text-left outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring/50"
                      >
                        {Icon ? (
                          <Icon className={cn("size-3.5 shrink-0", tone)} aria-hidden />
                        ) : (
                          <span className={cn("size-1.5 shrink-0 rounded-full border border-current", tone, filled && "bg-current")} aria-hidden />
                        )}
                        <span className="min-w-0 flex-1 overflow-hidden text-[12.5px] whitespace-nowrap [mask-image:linear-gradient(to_right,black_calc(100%-20px),transparent)]">{rowTitle(row.label)}</span>
                        {ref && <span className="hidden shrink-0 text-[10px] tabular-nums text-muted-foreground group-hover/row:block group-focus-visible/row:block">{ref}</span>}
                      </button>
                    </li>
                  );
                })}
                {(hidden > 0 || (all && group.rows.length > SECTION_ROWS_SHOWN)) && (
                  <li>
                    <button
                      type="button"
                      onClick={() => setExpanded((e) => (all ? e.filter((k) => k !== group.key) : [...e, group.key]))}
                      className="flex h-7 w-full items-center rounded-md px-1.5 pl-[18px] text-left text-[12.5px] text-muted-foreground outline-none hover:bg-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
                    >
                      {all ? "Show less" : `Show ${hidden} more`}
                    </button>
                  </li>
                )}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
});
