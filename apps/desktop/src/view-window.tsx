import { useEffect, useRef } from "react";
import ReactDOM from "react-dom/client";

import { ViewWindowRelay } from "@/lib/plugin-views";
import { tauriBus } from "@/lib/plugins";

/** A plugin view's own window: just its sandboxed frame, whose calls go to the main window's plugin host. */
function ViewWindow({ label }: { label: string }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const relay = new ViewWindowRelay(label, tauriBus(), ({ srcdoc, title }) => {
      // Sandboxed like the sidebar: an opaque origin, and on macOS and Linux no Tauri IPC in child frames.
      const frame = document.createElement("iframe");
      frame.sandbox.add("allow-scripts", "allow-forms");
      frame.title = title;
      frame.className = "size-full border-0 bg-transparent";
      frame.addEventListener("load", () => relay.loaded());
      frame.srcdoc = srcdoc;
      box.current?.replaceChildren(frame);
      return frame.contentWindow;
    });
    const onMessage = (event: MessageEvent) => relay.receive(event.source, event.data);
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
      relay.dispose();
    };
  }, [label]);
  return <div ref={box} className="h-full bg-surface" />;
}

export function renderViewWindow(label: string) {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<ViewWindow label={label} />);
}
