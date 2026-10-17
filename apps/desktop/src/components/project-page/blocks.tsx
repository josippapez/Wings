import { ArrowDownIcon, ArrowUpIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { IconButton } from "@/components/icon-button";
import { PluginBlock } from "@/components/plugin-block";
import { PluginGlyph } from "@/components/plugin-glyph";
import { Button } from "@/components/ui/button";
import type { PluginView } from "@/lib/api";
import { listBlocks } from "@/lib/plugin-blocks";
import type { PluginHost } from "@/lib/plugins";
import type { Block } from "@/lib/project-info";

import { cn } from "@/lib/utils";

import { messageOf, type Apply } from "./use-project-info";

/** A plugin's block on the page: a title bar with move and delete, then the plugin's page in its own frame. */
export function BlockItem({ projectId, item, plugins, epoch, host, visible, first, last, apply }: { projectId: string; item: Block; plugins: PluginView[]; epoch: number; host: PluginHost | null; visible: boolean; first: boolean; last: boolean; apply: Apply }) {
  const entry = listBlocks(plugins).find((b) => b.pluginId === item.pluginId && b.blockId === item.blockId);
  const [confirm, setConfirm] = useState(false);
  const act = (op: Parameters<Apply>[0]) => apply(op).catch((e) => void toast.error(messageOf(e)));
  const title = entry?.title ?? `Missing block · ${item.pluginId}/${item.blockId}`;
  return (
    <article id={`item-${item.id}`} data-block-id={item.id} aria-label={title} className="group/block flex flex-col gap-1.5">
      <header className="flex min-h-8 items-center gap-1.5 px-1">
        {entry && <PluginGlyph name={entry.icon} svg={entry.iconSvg} className="size-3.5 shrink-0 text-muted-foreground" />}
        <h3 className="min-w-0 flex-1 truncate text-[13px] font-medium text-muted-foreground">{title}</h3>
        <span className={cn("flex items-center transition-opacity", entry ? "opacity-0 group-focus-within/block:opacity-100 group-hover/block:opacity-100" : "opacity-100")}>
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
          epoch={epoch}
          host={host}
          pluginId={item.pluginId}
          blockId={item.blockId}
          projectId={projectId}
          instanceId={item.id}
          state={item.state}
          height={item.height}
          minHeight={entry.minHeight}
          visible={visible}
          onState={(state) => apply({ kind: "setBlockState", id: item.id, state }).then(() => {})}
          onHeight={(height) => void act({ kind: "setBlockHeight", id: item.id, height })}
        />
      ) : (
        <p className="rounded-xl border border-dashed border-hairline px-5 py-4 text-[13px] text-muted-foreground">The plugin for this block ({item.pluginId}) is off or no longer offers it. Turn it on in Settings, or remove the block.</p>
      )}
    </article>
  );
}
