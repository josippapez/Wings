import { errorMessage, mapLimit } from "wings-plugin-sdk/async";

/** The message Wings rejects `wings.exec` with when a program prints more than 8 MB. */
export const OUTPUT_TOO_LARGE = "Output is larger than 8 MB";
const isTooLarge = (error: unknown) => errorMessage(error).includes(OUTPUT_TOO_LARGE);

/** Files per git call. */
const BATCH_FILES = 40;
/** A file whose own patch is bigger than this shows as "Too large to show" and the rest of the diff stays. */
const MAX_FILE_BYTES = 1024 * 1024;

const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

/**
 * The patch of `items` from `run`, one call per `size` items. A call that comes back too large is split in
 * half and tried again; one item that is too large on its own becomes `tooLarge(item)`. Calls run `parallel`
 * at a time, and the patches come back in the order of `items`.
 */
export async function batched<T>(
  items: T[],
  run: (items: T[]) => Promise<string>,
  tooLarge: (item: T) => string,
  { size = BATCH_FILES, parallel = 4 }: { size?: number; parallel?: number } = {},
): Promise<string> {
  const fetch = async (part: T[]): Promise<string> => {
    try {
      return await run(part);
    } catch (error) {
      if (!isTooLarge(error)) throw error;
      if (part.length === 1) return tooLarge(part[0]);
      const half = Math.ceil(part.length / 2);
      return (await fetch(part.slice(0, half))) + (await fetch(part.slice(half)));
    }
  };
  const chunks: T[][] = [];
  for (let i = 0; i < items.length; i += size) chunks.push(items.slice(i, i + size));
  return (await mapLimit(chunks, parallel, fetch)).join("");
}

/** A file entry in git's own format that says why there is no diff. */
export function placeholder(head: string, text: string) {
  return `${head}@@ -1 +1 @@\n ${text}\n`;
}

/** The part of a file's patch before its first hunk: `diff --git`, `index`, `---` and `+++` lines. */
function headOf(chunk: string) {
  const at = chunk.search(/^@@ /m);
  return at < 0 ? chunk : chunk.slice(0, at);
}

/**
 * Keeps each file's patch under `file` bytes and the whole under `total`, by turning the biggest files into
 * "Too large to show (N MB)" entries. Binary files and files with no hunks are already small.
 */
export function fitPatch(patch: string, { file = MAX_FILE_BYTES, total }: { file?: number; total: number }) {
  const chunks = patch.split(/^(?=diff --git )/m);
  const sizes = chunks.map((c) => c.length);
  const shrunk = new Set<number>();
  const shrink = (i: number) => {
    chunks[i] = placeholder(headOf(chunks[i]), `Too large to show (${mb(sizes[i])})`);
    sizes[i] = chunks[i].length;
    shrunk.add(i);
  };
  sizes.forEach((s, i) => s > file && shrink(i));
  let sum = sizes.reduce((n, s) => n + s, 0);
  for (const i of sizes.map((_, at) => at).sort((a, b) => sizes[b] - sizes[a])) {
    if (sum <= total) break;
    const before = sizes[i];
    if (!shrunk.has(i)) shrink(i);
    sum -= before - sizes[i];
  }
  return chunks.join("");
}
