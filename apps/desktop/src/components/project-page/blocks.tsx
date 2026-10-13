import { ArrowDownIcon, ArrowUpIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import { IconButton } from "@/components/icon-button";
import { PluginBlock } from "@/components/plugin-block";
import { PluginGlyph } from "@/components/plugin-glyph";
import { Button } from "@/components/ui/button";
import type { PluginView } from "@/lib/api";
import { listBlocks } from "@/lib/plugin-blocks";
import type { PluginHost } from "@/lib/plugins";
import type { Block } from "@/lib/project-info";

import { messageOf, type Apply } from "./use-project-info";

/** A plugin's block on the page: a title bar with move and delete, then the plugin's page in its own frame. */
export function BlockItem({ projectId, item, plugins, host, visible, first, last, apply }: { projectId: string; item: Block; plugins: PluginView[]; host: PluginHost | null; visible: boolean; first: boolean; last: boolean; apply: Apply }) {
  const entry = listBlocks(plugins).find((b) => b.pluginId === item.pluginId && b.blockId === item.blockId);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState("");
  const act = (op: Parameters<Apply>[0]) => apply(op).then(() => setError(""), (e) => setError(messageOf(e)));
  const title = entry?.title ?? item.blockId;
  return (
    <article id={`item-${item.id}`} data-block-id={item.id} aria-label={title} className="group/block flex flex-col gap-1.5">
      <header className="flex items-center gap-1.5 px-1">
        {entry && <PluginGlyph name={entry.icon} svg={entry.iconSvg} className="size-3.5 shrink-0 text-muted-foreground" />}
        <h3 className="min-w-0 flex-1 truncate text-[13px] font-semibold text-muted-foreground">{title}</h3>
        <span className="flex items-center opacity-0 transition-opacity group-focus-within/block:opacity-100 group-hover/block:opacity-100">
          <IconButton label={`Move ${title} up`} disabled={first} onClick={() => void act({ kind: "moveItem", id: item.id, dir: -1 })}>
            <ArrowUpIcon />
          </IconButton>
          <IconButton label={`Move ${title} down`} disabled={last} onClick={() => void act({ kind: "moveItem", id: item.id, dir: 1 })}>
            <ArrowDownIcon />
          </IconButton>
          {confirm ? (
            <>
              <Button size="xs" variant="destructive" autoFocus onClick={() => void act({ kind: "deleteItem", id: item.id })}>
                Remove block
              </Button>
              <Button size="xs" variant="ghost" onClick={() => setConfirm(false)}>
                Keep
              </Button>
            </>
          ) : (
            <IconButton label={`Remove ${title}`} onClick={() => setConfirm(true)}>
              <Trash2Icon />
            </IconButton>
          )}
        </span>
      </header>
      {entry ? (
        <PluginBlock
          plugins={plugins}
          host={host}
          pluginId={item.pluginId}
          blockId={item.blockId}
          projectId={projectId}
          instanceId={item.id}
          state={item.state}
          height={item.height}
          minHeight={entry.minHeight}
          visible={visible}
          onState={(state) => apply({ kind: "setBlockState", id: item.id, state }).then(() => setError(""))}
          onHeight={(height) => void act({ kind: "setBlockHeight", id: item.id, height })}
        />
      ) : (
        <p className="rounded-xl border border-dashed border-hairline p-4 text-[13px] text-muted-foreground">The plugin for this block ({item.pluginId}) is off or no longer offers it. Turn it on in Settings, or remove the block.</p>
      )}
      {error && (
        <p role="alert" className="px-1 text-[12px] text-destructive">
          {error}
        </p>
      )}
    </article>
  );
}
