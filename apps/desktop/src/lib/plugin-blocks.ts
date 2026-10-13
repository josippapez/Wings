import type { PluginView } from "@/lib/api";

/** What `wings.setBlockState` may save per block, as JSON text. */
export const BLOCK_STATE_MAX = 16 * 1024;
/** A block's least height when its manifest leaves `minHeight` out, and the bounds a manifest may set. */
export const BLOCK_MIN_HEIGHT_DEFAULT = 160;
export const BLOCK_MIN_HEIGHT_RANGE = [80, 800] as const;
/** The most `wings.setHeight` may ask for. */
export const BLOCK_MAX_HEIGHT = 1200;
/** Block frames live at once on one Project page; each is a whole document. */
export const MAX_LIVE_BLOCKS = 8;
/** A block that stays out of view this long unloads, like a sidebar. */
export const BLOCK_UNLOAD_MS = 5 * 60_000;

/** `state` as JSON text if it is JSON and fits `BLOCK_STATE_MAX` UTF-8 bytes, the unit Rust counts; throws a message otherwise. */
export function blockStateJson(state: unknown) {
  const text = JSON.stringify(state);
  if (text === undefined) throw new Error("Block state must be a JSON value");
  if (new TextEncoder().encode(text).length > BLOCK_STATE_MAX) throw new Error(`Block state is limited to ${BLOCK_STATE_MAX / 1024} KB`);
  return text;
}

/** `px` as a whole number of pixels within `min` to `BLOCK_MAX_HEIGHT`. Throws for something that isn't a finite number. */
export function clampBlockHeight(px: unknown, min: number) {
  if (typeof px !== "number" || !Number.isFinite(px)) throw new Error("wings.setHeight needs a number of pixels");
  return Math.min(BLOCK_MAX_HEIGHT, Math.max(min, Math.round(px)));
}

/** A block as the Project page's "+ Add" menu offers it. `key` is `pluginId:blockId`. */
export type BlockEntry = { key: string; pluginId: string; blockId: string; title: string; icon: string; iconSvg: string | null; minHeight: number };

/** The blocks of every plugin that is on and approved, in plugin order. */
export const listBlocks = (plugins: PluginView[]): BlockEntry[] =>
  plugins
    .filter((p) => p.enabled && p.approved)
    .flatMap((p) =>
      (p.contributes.blocks ?? []).map((b) => ({ key: `${p.id}:${b.id}`, pluginId: p.id, blockId: b.id, title: b.title, icon: b.icon, iconSvg: p.iconSvg, minHeight: b.minHeight ?? BLOCK_MIN_HEIGHT_DEFAULT })),
    );

/** One block of a plugin by id, with its `minHeight` filled in; undefined if the plugin doesn't have it. */
export function findBlock(plugin: PluginView, blockId: string) {
  const block = (plugin.contributes.blocks ?? []).find((b) => b.id === blockId);
  return block && { ...block, minHeight: block.minHeight ?? BLOCK_MIN_HEIGHT_DEFAULT };
}

/** What a plugin page needs to run as a block: the project it shows and the block's saved state. */
export type BlockMount = {
  spaceId: string;
  blockId: string;
  instanceId: string;
  minHeight: number;
  /** Out of view when it mounts: its `wings.focused` is false from its first line. */
  hidden: boolean;
  /** The saved state, as `wings.blockState` first returns it. */
  state: unknown;
  /** A page saved new state (already checked: JSON, within `BLOCK_STATE_MAX` bytes). Resolves once it is stored, rejects with why not. */
  onState: (state: unknown) => Promise<void>;
  /** A page asked for a height (already clamped). */
  onHeight: (px: number) => void;
};
