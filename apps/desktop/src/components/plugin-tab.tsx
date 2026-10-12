import { useEffect, useRef } from "react";

import type { PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";

/**
 * A plugin view open as a tab: its page fills the pane area like a pane card. It stays mounted while another tab
 * shows, so it keeps running, and goes when the tab closes or `loaded` goes false (the tab was hidden long enough
 * to unload; the page loads again when the tab is shown). `pageKey` changes when the plugin restarts.
 */
export function PluginTab(props: { plugin: PluginView | undefined; viewId: string; pageKey: string; visible: boolean; loaded: boolean; host: PluginHost | null }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const { plugin, host } = props;
    if (!box.current || !plugin || !host || !props.loaded) return;
    let remove: (() => void) | undefined;
    let gone = false;
    void host
      .mountPage(plugin, props.viewId, box.current, "tab")
      .then((r) => (gone ? r() : (remove = r)))
      .catch((e) => console.error(e));
    return () => {
      gone = true;
      remove?.();
    };
  }, [props.pageKey, props.host, props.loaded]);
  return (
    <div className={cn("absolute inset-0 flex p-2 pt-0", !props.visible && "invisible")} aria-hidden={!props.visible}>
      <div ref={box} data-plugin-tab={props.pageKey} className="min-h-0 min-w-0 flex-1 overflow-hidden rounded-[14px] border border-hairline bg-surface" />
    </div>
  );
}
