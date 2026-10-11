import { useEffect, useRef } from "react";
import { AppWindowIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { pagesOf, type PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";

/** `id` is `pluginId:viewId`; `key` also changes when the plugin restarts, so its page loads again. */
export type ModalRef = { plugin: PluginView; viewId: string; id: string; key: string };

/** The manifest's size, or a default, within bounds. CSS keeps it on screen too. */
const bound = (value: number | null, fallback: number, min: number, max: number) => Math.min(Math.max(value ?? fallback, min), max);

/** The plugin's page, removed when the modal closes: unlike a sidebar, a closed modal keeps nothing running. */
function Page({ entry, host }: { entry: ModalRef; host: PluginHost }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!box.current) return;
    let remove: (() => void) | undefined;
    let gone = false;
    void host
      .mountPage(entry.plugin, entry.viewId, box.current, "modal")
      .then((r) => (gone ? r() : (remove = r)))
      .catch((e) => console.error(e));
    return () => {
      gone = true;
      remove?.();
    };
  }, [entry.key, host]);
  return <div ref={box} className="min-h-0 flex-1" />;
}

/** A plugin page in a dialog over Wings, sized by its manifest. */
export function PluginModal(props: {
  open: ModalRef | null;
  host: PluginHost | null;
  onClose: () => void;
  /** Moves the page to a window of its own. Offered for pages whose manifest allows it. */
  onPopOut: (entry: ModalRef) => void;
}) {
  // The last page shown stays while the dialog animates out.
  const last = useRef(props.open);
  if (props.open) last.current = props.open;
  const shown = props.open ?? last.current;
  const page = shown ? pagesOf(shown.plugin).find((v) => v.id === shown.viewId) : undefined;
  const width = bound(page?.width ?? null, 560, 320, 1200);
  const height = bound(page?.height ?? null, 480, 200, 900);
  return (
    <Dialog open={props.open !== null} onOpenChange={(open) => !open && props.onClose()}>
      <DialogContent
        showCloseButton={false}
        className="flex max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none"
        style={{ width: `min(${width}px, calc(100vw - 4rem))`, height: `min(${height}px, calc(100vh - 6rem))` }}
      >
        <header className="flex h-10 shrink-0 items-center gap-1 border-b border-hairline pr-2 pl-4">
          <DialogTitle className="min-w-0 flex-1 truncate text-[13px] font-medium">{page?.title}</DialogTitle>
          {shown && page?.opens.includes("window") && (
            <IconButton label="Open in a window" onClick={() => props.onPopOut(shown)} className="size-7">
              <AppWindowIcon />
            </IconButton>
          )}
          <IconButton label="Close" onClick={props.onClose} className="size-7">
            <XIcon />
          </IconButton>
        </header>
        {shown && props.host && <Page key={shown.key} entry={shown} host={props.host} />}
      </DialogContent>
    </Dialog>
  );
}
