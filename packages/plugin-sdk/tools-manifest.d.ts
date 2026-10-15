/** A tool as `contributes.mcpTools` in wings-plugin.json lists it. */
export interface McpTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

/**
 * The tools of the plugin in `dir`: what its `src/tools.ts` exports as `tools`, in the order it lists them. Resolves
 * null when the plugin has no `src/tools.ts`. The file runs in Node, with a `wings` that does nothing, so it and
 * what it imports must not need Wings when they load.
 */
export function readTools(dir: string): Promise<McpTool[] | null>;

/**
 * Writes the tools of `src/tools.ts` as `contributes.mcpTools` in the plugin's `wings-plugin.json`, and leaves the
 * rest of the file as it is. A manifest that already lists them is not touched. With `check`, writes nothing and
 * lists in `stale` the manifest if it would change. With `outDir`, writes the manifest there, from the one in
 * `dir`. A plugin without a `src/tools.ts` is left alone.
 */
export function syncTools(
  dir: string,
  options?: { check?: boolean; outDir?: string },
): Promise<{ built: { file: string; bytes: number }[]; stale: string[] }>;
