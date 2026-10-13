// The Project page's data, kept by Wings (src-tauri/src/project_info.rs): facts, sections and plugin blocks per
// project. Every change is an `Op` the backend applies to what is stored at that moment, and it emits
// `project-info-changed` after each one (from the page or from an agent's tool), so a page refetches on that.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const LIMITS = {
  facts: 40,
  key: 60,
  value: 500,
  sections: 30,
  title: 80,
  body: 20_000,
  blocks: 8,
  blockState: 16 * 1024,
  minHeight: 80,
  maxHeight: 2000,
  image: 5 * 1024 * 1024,
} as const;

export type Who = "you" | "agent";

export type Fact = { key: string; value: string; by: Who; updatedAt: number };

export type Section = { kind: "section"; id: string; title: string; /** Markdown. */ body: string; /** Who last wrote it. */ by: Who; updatedAt: number };

/** A plugin's live view placed on the page. Its `state` and `height` are saved with SetBlockState / SetBlockHeight. */
export type Block = { kind: "block"; id: string; pluginId: string; blockId: string; state: unknown | null; height: number | null };

/** `items` is the page's order. */
export type Item = Section | Block;

export type Info = { v: 1; facts: Fact[]; items: Item[]; updatedAt: number };

/** What an edit started from: the UTF-16 length and fingerprint of the section's body when the page last read it. */
export type Base = { length: number; hash: number };

/** FNV-1a over UTF-16 code units, the same as the backend's. */
export function fingerprint(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return h >>> 0;
}

export const baseOf = (text: string): Base => ({ length: text.length, hash: fingerprint(text) });

export type Op =
  /** `oldKey` is the key being renamed, when a row's key is edited. `by` defaults to "you". */
  | { kind: "setFact"; key: string; value: string; by?: Who; oldKey?: string }
  | { kind: "removeFact"; key: string; by?: Who }
  | { kind: "writeSection"; title: string; body: string; by?: Who }
  | { kind: "appendSection"; title: string; body: string; by?: Who }
  /** A user's edit. What an agent appended after `baseLen` stays after the new body. Leave `title` or `body` out to keep it. */
  | { kind: "editSection"; id: string; title?: string; body?: string; baseLen: number; baseHash: number }
  /** Added after item `after`, or at the end. */
  | { kind: "addSection"; title: string; after?: string }
  | { kind: "addBlock"; pluginId: string; blockId: string; after?: string }
  | { kind: "setBlockState"; id: string; state: unknown | null }
  | { kind: "setBlockHeight"; id: string; height: number }
  | { kind: "moveItem"; id: string; dir: -1 | 1 }
  | { kind: "deleteItem"; id: string };

/** An edit of a section's body, from the text the page started from. */
export const editSection = (id: string, body: string, from: string, title?: string): Op => ({
  kind: "editSection",
  id,
  title,
  body,
  baseLen: from.length,
  baseHash: fingerprint(from),
});

/** In Markdown, an image stored with `addImage` is written `![alt](wings-image:<name>)`. */
export const IMAGE_SCHEME = "wings-image:";

const urls = new Map<string, Promise<string>>();

export const projectInfo = {
  /** Reads a project's info. The first read of a project brings over the Notes plugin's facts and sections. */
  get: (projectId: string) => invoke<Info>("project_info_get", { projectId }),
  /** Applies one change and returns the whole info after it. Rejects with a message to show when a limit is hit. */
  apply: (projectId: string, op: Op) => invoke<Info>("project_info_apply", { projectId, op }),
  /** Stores a png, jpeg, gif or webp (up to 5 MB) and returns its name, for `![](wings-image:<name>)`. */
  addImage: (projectId: string, bytes: Uint8Array | ArrayBuffer, ext: string) =>
    invoke<string>("project_info_add_image", bytes, { headers: { "x-project-id": encodeURIComponent(projectId), "x-ext": ext } }),
  /** A blob: URL for a stored image, usable as `<img src>`. Cached per image; rejects when the image is missing. */
  imageUrl: (projectId: string, name: string): Promise<string> => {
    const key = `${projectId}\0${name}`;
    let url = urls.get(key);
    if (!url) {
      url = invoke<ArrayBuffer>("project_info_image", { projectId, name }).then((bytes) => URL.createObjectURL(new Blob([bytes])));
      url.catch(() => urls.delete(key));
      urls.set(key, url);
    }
    return url;
  },
  /** Calls `fn` with the project that changed, whoever changed it. */
  onChanged: (fn: (projectId: string) => void): Promise<UnlistenFn> =>
    listen<{ projectId: string }>("project-info-changed", (e) => fn(e.payload.projectId)),
};
