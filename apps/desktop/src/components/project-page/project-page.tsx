import { FolderIcon, GitBranchIcon } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";

import type { PluginView } from "@/lib/api";
import { listBlocks, type BlockEntry } from "@/lib/plugin-blocks";
import type { PluginHost } from "@/lib/plugins";
import { LIMITS, projectInfo } from "@/lib/project-info";

import { AddRow } from "./add-row";
import { BlockItem } from "./blocks";
import { FactsTable } from "./facts";
import { freshTitle } from "./merge";
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
  const addRow = (after: string | undefined, label: string) => (
    <AddRow
      label={label}
      blocks={blocks}
      canSection={sections.length < LIMITS.sections}
      canBlock={blockCount < LIMITS.blocks}
      onSection={() => void add({ kind: "addSection", title: freshTitle(sections.map((s) => s.title)), after })}
      onFact={() => factInput.current && (factInput.current.scrollIntoView({ block: "center" }), factInput.current.focus())}
      onBlock={(b: BlockEntry) => void add({ kind: "addBlock", pluginId: b.pluginId, blockId: b.blockId, after })}
    />
  );

  return (
    <div ref={page} className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-6 py-8">
      <header className="flex flex-col gap-1">
        <h1 className="text-[22px] font-semibold tracking-tight">{space.name}</h1>
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
      <FactsTable facts={info.facts} apply={apply} addRef={factInput} />
      {info.items.length === 0 && <p className="mt-2 text-[13px] text-muted-foreground">Nothing written about {space.name} yet. Add a section below, or ask Claude to keep what it learns here.</p>}
      <div className="flex flex-col">
        {info.items.map((item, i) => (
          <Fragment key={item.id}>
            {item.kind === "section" ? (
              <SectionCard projectId={space.id} section={item} first={i === 0} last={i === info.items.length - 1} startEditing={fresh.current === item.id} apply={apply} read={read} />
            ) : (
              <BlockItem projectId={space.id} item={item} plugins={plugins} host={host} visible={visible && onScreen.has(item.id)} first={i === 0} last={i === info.items.length - 1} apply={apply} />
            )}
            {addRow(item.id, `Add after ${item.kind === "section" ? item.title : item.blockId}`)}
          </Fragment>
        ))}
        {info.items.length === 0 && addRow(undefined, "Add")}
      </div>
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
