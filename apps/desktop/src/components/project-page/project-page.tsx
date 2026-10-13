import { FolderIcon, GitBranchIcon, SearchIcon } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { PluginView } from "@/lib/api";
import { listBlocks, type BlockEntry } from "@/lib/plugin-blocks";
import type { PluginHost } from "@/lib/plugins";
import { LIMITS, projectInfo, type Place } from "@/lib/project-info";
import { cn } from "@/lib/utils";

import { AddRow } from "./add-row";
import { BlockItem } from "./blocks";
import { FactsTable } from "./facts";
import { freshTitle } from "./merge";
import { clampMatch, findMatches, matchId, PROJECT_PAGE_FIND, stepMatch, markClass, type Mark } from "./search";
import { SectionCard } from "./sections";
import { messageOf, useProjectInfo } from "./use-project-info";

export type ProjectPageSpace = { id: string; name: string; path: string };

/** A project's page: its facts, then sections and plugin blocks in the order you put them, with "+ Add" between. */
export function ProjectPage({ space, branch, plugins, host, visible }: { space: ProjectPageSpace; branch?: string | null; plugins: PluginView[]; host: PluginHost | null; visible: boolean }) {
  const { info, error: unreadable, apply } = useProjectInfo(space.id);
  const [error, setError] = useState("");
  const factInput = useRef<HTMLInputElement>(null);
  /** The section just added, which opens for writing. */
  const fresh = useRef<string | null>(null);
  const page = useRef<HTMLDivElement>(null);
  /** Blocks on screen, within 200px: one observer for the page, so a block off screen is told it is hidden, stops polling and unloads. */
  const [onScreen, setOnScreen] = useState<ReadonlySet<string>>(new Set());
  const blockIds = (info?.items ?? []).flatMap((i) => (i.kind === "block" ? [i.id] : [])).join(" ");
  useEffect(() => {
    const root = page.current?.closest<HTMLElement>("[data-project-scroll]") ?? null;
    const nodes = [...(page.current?.querySelectorAll<HTMLElement>("[data-block-id]") ?? [])];
    if (!nodes.length) return;
    const observer = new IntersectionObserver(
      (entries) =>
        setOnScreen((prev) => {
          const next = new Set(prev);
          for (const e of entries) {
            const id = (e.target as HTMLElement).dataset.blockId!;
            if (e.isIntersecting) next.add(id);
            else next.delete(id);
          }
          return next.size === prev.size && [...next].every((id) => prev.has(id)) ? prev : next;
        }),
      { root, rootMargin: "200px" },
    );
    nodes.forEach((n) => observer.observe(n));
    return () => observer.disconnect();
  }, [blockIds]);

  // Search: typing costs a render of the input only; the page is searched 150 ms after typing stops.
  const searchInput = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [applied, setApplied] = useState("");
  const [at, setAt] = useState(0);
  useEffect(() => {
    const t = setTimeout(() => (setApplied(query), setAt(0)), 150);
    return () => clearTimeout(t);
  }, [query]);
  const matches = useMemo(() => (info ? findMatches(info, applied) : []), [info, applied]);
  const shown = clampMatch(at, matches.length);
  const marks = useMemo(() => {
    const m = new Map<string, Mark>();
    matches.forEach((x, i) => m.set(matchId(x), i === shown ? "current" : "hit"));
    return m;
  }, [matches, shown]);
  const factMarks = useMemo(() => new Map([...marks].filter(([k]) => k.startsWith("fact:")).map(([k, v]) => [k.slice(5), v])), [marks]);
  const current = matches[shown];
  const currentKey = current && matchId(current);
  useEffect(() => {
    if (!currentKey) return;
    const el = currentKey.startsWith("item:")
      ? page.current?.querySelector(`#item-${CSS.escape(currentKey.slice(5))}`)
      : page.current?.querySelector(`[data-fact-key="${CSS.escape(currentKey.slice(5))}"]`);
    el?.scrollIntoView({ block: "center" });
  }, [currentKey]);
  useEffect(() => {
    if (!visible) return;
    const focus = (e: Event) => {
      if ((e as CustomEvent<string>).detail !== space.id) return;
      searchInput.current?.focus();
      searchInput.current?.select();
    };
    window.addEventListener(PROJECT_PAGE_FIND, focus);
    return () => window.removeEventListener(PROJECT_PAGE_FIND, focus);
  }, [visible, space.id]);
  const read = useCallback((name: string) => projectInfo.imageUrl(space.id, name), [space.id]);
  const blocks = listBlocks(plugins);

  const add = (op: Parameters<typeof apply>[0]) =>
    apply(op, (next, before) => {
      const known = new Set(before?.items.map((i) => i.id));
      const added = next.items.find((i) => !known.has(i.id));
      fresh.current = added?.kind === "section" ? added.id : null;
    }).then(
      () => setError(""),
      (e) => setError(messageOf(e)),
    );

  if (unreadable && !info) {
    return (
      <div role="alert" className="p-6 text-[13px] text-muted-foreground">
        The page for {space.name} can't be read: {unreadable}
      </div>
    );
  }
  if (!info) return <div role="status" className="p-6 text-[13px] text-muted-foreground">Loading the project page</div>;

  const sections = info.items.filter((i) => i.kind === "section");
  const blockCount = info.items.length - sections.length;
  const addRow = (at: Place, label: string) => (
    <AddRow
      label={label}
      blocks={blocks}
      canSection={sections.length < LIMITS.sections}
      canBlock={blockCount < LIMITS.blocks}
      onSection={() => void add({ kind: "addSection", title: freshTitle(sections.map((s) => s.title)), at })}
      onFact={() => factInput.current && (factInput.current.scrollIntoView({ block: "center" }), factInput.current.focus())}
      onBlock={(b: BlockEntry) => void add({ kind: "addBlock", pluginId: b.pluginId, blockId: b.blockId, at })}
    />
  );

  return (
    <div ref={page} className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-6 py-8">
      <header className="flex flex-col gap-1">
        <div className="flex items-center gap-3">
          <h1 className="min-w-0 flex-1 truncate text-[22px] font-semibold tracking-tight">{space.name}</h1>
          <div role="search" className="flex items-center gap-2">
            <label className="relative flex items-center">
              <SearchIcon className="pointer-events-none absolute left-2 size-3.5 text-muted-foreground" aria-hidden />
              <input
                ref={searchInput}
                type="search"
                aria-label="Search this page"
                placeholder="Search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    setQuery("");
                    setApplied("");
                    e.currentTarget.blur();
                  } else if (e.key === "Enter") {
                    e.preventDefault();
                    // Enter right after typing searches now instead of waiting out the pause.
                    if (query !== applied) setApplied(query);
                    else setAt(() => stepMatch(shown, e.shiftKey ? -1 : 1, matches.length));
                  }
                }}
                className="h-7 w-44 rounded-lg border border-input bg-transparent pr-2 pl-7 text-[12px] outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              />
            </label>
            {applied.trim() !== "" && (
              <span role="status" className="text-[11px] whitespace-nowrap text-muted-foreground">
                {matches.length === 0 ? "No matches" : `${shown + 1} of ${matches.length} ${matches.length === 1 ? "match" : "matches"}`}
              </span>
            )}
          </div>
        </div>
        <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-muted-foreground">
          <span className="inline-flex items-center gap-1" title={space.path}>
            <FolderIcon className="size-3.5" aria-hidden />
            {space.path.replace(/^\/Users\/[^/]+/, "~")}
          </span>
          {branch && (
            <span className="inline-flex items-center gap-1">
              <GitBranchIcon className="size-3.5" aria-hidden />
              {branch}
            </span>
          )}
        </p>
      </header>
      <FactsTable facts={info.facts} apply={apply} addRef={factInput} marks={factMarks} />
      {info.items.length === 0 && <p className="mt-2 text-[13px] text-muted-foreground">Nothing written about {space.name} yet. Add a section below, or ask Claude to keep what it learns here.</p>}
      <div className="flex flex-col">
        {addRow("top", info.items.length === 0 ? "Add" : "Add at the top")}
        {info.items.map((item, i) => (
          <Fragment key={item.id}>
            {item.kind === "section" ? (
              <SectionCard mark={marks.get(`item:${item.id}`)} projectId={space.id} section={item} first={i === 0} last={i === info.items.length - 1} startEditing={fresh.current === item.id} apply={apply} read={read} />
            ) : (
              <div className={cn("rounded-xl", markClass(marks.get(`item:${item.id}`)))}>
                <BlockItem projectId={space.id} item={item} plugins={plugins} host={host} visible={visible && onScreen.has(item.id)} first={i === 0} last={i === info.items.length - 1} apply={apply} />
              </div>
            )}
            {addRow({ after: item.id }, `Add after ${item.kind === "section" ? item.title : item.blockId}`)}
          </Fragment>
        ))}
      </div>
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
