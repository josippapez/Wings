import { useEffect, useSyncExternalStore } from "react";
import { toast } from "sonner";

import { api, type Editor } from "@/lib/api";

const KEY = "wings.defaultEditor";
const NONE: Editor[] = [];
const listeners = new Set<() => void>();
let installed: Editor[] | null = null;
let loading = false;

const changed = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

/** The editors found on this computer, looked up once and kept until the app restarts. */
export function useEditors(): Editor[] {
  useEffect(() => {
    if (loading) return;
    loading = true;
    api
      .editorsList()
      .then((list) => {
        installed = list;
        changed();
      })
      .catch((e) => {
        loading = false;
        console.error(e);
      });
  }, []);
  return useSyncExternalStore(subscribe, () => installed) ?? NONE;
}

/** The editor chosen in Settings, else the first one found. */
export function defaultEditor(list: Editor[], chosen: string | null): Editor | undefined {
  return list.find((e) => e.id === chosen) ?? list[0];
}

/** The id chosen in Settings (null before a choice) and a setter. Kept in localStorage, which a WebView holds per app. */
export function useDefaultEditorId(): [string | null, (id: string) => void] {
  const id = useSyncExternalStore(subscribe, () => localStorage.getItem(KEY));
  return [
    id,
    (next) => {
      localStorage.setItem(KEY, next);
      changed();
    },
  ];
}

export function openInEditor(editor: Editor, spaceId: string) {
  api.editorOpen(editor.id, spaceId).catch((e) => toast.error(`Couldn't open ${editor.name}`, { description: String(e) }));
}
