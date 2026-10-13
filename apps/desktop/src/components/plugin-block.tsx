import { useEffect, useRef, useState } from "react";

import { PluginGlyph } from "@/components/plugin-glyph";
import { Spinner } from "@/components/ui/spinner";
import type { PluginView } from "@/lib/api";
import { BLOCK_MAX_HEIGHT, BLOCK_UNLOAD_MS, findBlock } from "@/lib/plugin-blocks";
import type { PluginHost } from "@/lib/plugins";

/** How long a page settles on a height before it is saved; the frame itself follows at once. */
const SAVE_HEIGHT_MS = 400;

export type PluginBlockProps = {
  plugins: PluginView[];
  host: PluginHost | null;
  pluginId: string;
  blockId: string;
  /** The project the page shows; `wings.project()` in the block. */
  projectId: string;
  /** This block's own id on the Project page. Two blocks of one page have separate state. */
  instanceId: string;
  /** What `wings.setBlockState` saved last, which `wings.blockState()` gives the page when it loads. */
  state: unknown;
  /** The saved height in CSS pixels, if the page ever set one. */
  height: number | null | undefined;
  minHeight: number;
  /** On screen now: the page passes what an IntersectionObserver says, false too while its tab is hidden. */
  visible: boolean;
  /** Store the state; resolve once it is saved, reject with the error if not. The page's `setBlockState` waits for it. */
  onState: (state: unknown) => Promise<void>;
  onHeight: (px: number) => void;
};

/**
 * One block of a Project page: the plugin's page in a sandboxed frame, fixed to the page's project. The frame mounts the
 * first time `visible` is true, is told it's out of view while `visible` is false (so its polls pause), and unloads after
 * `BLOCK_UNLOAD_MS` hidden; it loads again, with its saved state, when `visible` returns. A plugin that is off or
 * not approved shows a placeholder instead.
 */
export function PluginBlock(props: PluginBlockProps) {
  const { plugins, host, pluginId, blockId, projectId, instanceId, visible } = props;
  const plugin = plugins.find((p) => p.id === pluginId);
  const runnable = !!plugin && plugin.enabled && plugin.approved;
  const block = plugin && findBlock(plugin, blockId);
  const title = block?.title ?? blockId;
  const box = useRef<HTMLDivElement>(null);
  const latest = useRef(props);
  latest.current = props;
  const [loaded, setLoaded] = useState(visible);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hidden = useRef<((hidden: boolean) => void) | null>(null);
  // Set once: React never writes the height again, so a page's own height isn't undone by a re-render with the saved one.
  const [start] = useState(() => Math.min(BLOCK_MAX_HEIGHT, Math.max(props.minHeight, props.height ?? props.minHeight)));

  useEffect(() => {
    if (visible) return setLoaded(true);
    const timer = setTimeout(() => setLoaded(false), BLOCK_UNLOAD_MS);
    return () => clearTimeout(timer);
  }, [visible]);

  useEffect(() => hidden.current?.(!visible), [visible]);

  useEffect(() => {
    if (!loaded || !runnable || !host || !plugin || !block || !box.current) return;
    const container = box.current;
    let gone = false;
    let remove: (() => void) | undefined;
    let frame = 0;
    let saveTimer: ReturnType<typeof setTimeout> | undefined;
    let saving: number | null = null;
    const save = () => {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      if (saving !== null) latest.current.onHeight(saving);
      saving = null;
    };
    setReady(false);
    setError(null);
    host
      .mountBlock(plugin, container, {
        spaceId: projectId,
        blockId,
        instanceId,
        minHeight: block.minHeight,
        hidden: !latest.current.visible,
        state: latest.current.state,
        onState: (state) => latest.current.onState(state),
        onHeight: (px) => {
          if (gone) return;
          // Several asks in a frame make one style write, and the save waits until the page settles.
          cancelAnimationFrame(frame);
          frame = requestAnimationFrame(() => (container.style.height = `${px}px`));
          saving = px;
          clearTimeout(saveTimer);
          saveTimer = setTimeout(save, SAVE_HEIGHT_MS);
        },
      })
      .then((mounted) => {
        if (gone) return mounted.remove();
        remove = mounted.remove;
        hidden.current = mounted.setHidden;
        mounted.setHidden(!latest.current.visible);
        setReady(true);
      })
      .catch((e) => !gone && setError(String(e instanceof Error ? e.message : e)));
    return () => {
      gone = true;
      cancelAnimationFrame(frame);
      save();
      hidden.current = null;
      remove?.();
    };
  }, [loaded, runnable, host, plugin?.version, pluginId, blockId, projectId, instanceId, block?.minHeight]);

  const note = !plugin ? `${pluginId} isn't installed` : !runnable ? `Turn on ${plugin.name} to show this` : !block ? `${plugin.name} no longer has this block` : error;
  const waiting = runnable && !error && !ready;
  return (
    <section aria-label={title} data-plugin-block={instanceId} className="relative overflow-hidden rounded-[14px] border border-hairline bg-surface">
      <div ref={box} style={{ height: start }} className="w-full" />
      {(note || waiting) && (
        <div className="absolute inset-0 flex items-center justify-center gap-2 bg-surface px-4 text-center text-xs text-muted-foreground">
          {waiting ? <Spinner /> : <PluginGlyph name={block?.icon ?? ""} svg={plugin?.iconSvg} className="size-3.5 shrink-0" />}
          <span className="min-w-0 truncate">{waiting ? title : note}</span>
        </div>
      )}
    </section>
  );
}
