import { useEffect, useRef, useState } from "react";

import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import type { PluginView } from "@/lib/api";
import type { PageTab, PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";

/**
 * A plugin view open as a tab: its page fills the pane area like a pane card. It stays mounted while another tab
 * shows, so it keeps running (its page is told it is out of view, so its polls pause), and goes when the tab closes or `loaded` goes false (the tab was hidden long enough
 * to unload; the page loads again when the tab is shown). `pageKey` changes when the plugin restarts or the tab gets new args; `tab` is read when the page mounts.
 */
export function PluginTab(props: { plugin: PluginView | undefined; viewId: string; pageKey: string; tab: PageTab; visible: boolean; loaded: boolean; host: PluginHost | null }) {
  const box = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");
  const page = useRef<{ setHidden: (hidden: boolean) => void }>(null);
  // Read when the page finishes mounting, which can be after the tab was shown or hidden.
  const hidden = useRef(!props.visible);
  hidden.current = !props.visible;
  useEffect(() => page.current?.setHidden(!props.visible), [props.visible]);
  useEffect(() => {
    const { plugin, host } = props;
    if (!box.current || !plugin || !host || !props.loaded) return;
    setError("");
    let remove: (() => void) | undefined;
    let gone = false;
    void host
      .mountPage(plugin, props.viewId, box.current, "tab", props.tab, hidden.current)
      .then((r) => {
        if (gone) return r.remove();
        remove = r.remove;
        page.current = r;
        r.setHidden(hidden.current);
      })
      .catch((e) => setError(String(e)));
    return () => {
      gone = true;
      page.current = null;
      remove?.();
    };
  }, [props.pageKey, props.host, props.loaded]);
  return (
    <div className={cn("absolute inset-0 flex p-2 pt-0", !props.visible && "invisible")} aria-hidden={!props.visible}>
      <div ref={box} data-plugin-tab={props.pageKey} className="min-h-0 min-w-0 flex-1 overflow-hidden rounded-[14px] border border-hairline bg-surface">
        {error && (
          <Empty className="size-full">
            <EmptyHeader>
              <EmptyTitle>This page couldn't load</EmptyTitle>
              <EmptyDescription>{error}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
      </div>
    </div>
  );
}
