import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isViewLabel } from "@/lib/plugin-views";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Linux has no OS blur behind the window, so the theme swaps the glass for opaque colors there.
const ua = navigator.userAgent;
document.documentElement.dataset.platform = /Mac/.test(ua) ? "macos" : /Windows/.test(ua) ? "windows" : "linux";

// WebKit's own menu offers only "Reload", which reloads the whole UI. Tauri has no switch for it on macOS or Linux
// (wry's `with_default_context_menus` is WebView2 only). Inputs and a terminal keep it for Copy and Paste.
document.addEventListener("contextmenu", (e) => {
  if (e.target instanceof Element && e.target.closest("input, textarea, [contenteditable], .xterm")) return;
  e.preventDefault();
});

// No StrictMode: its double-mount in dev would spawn every terminal's shell twice.
const renderApp = () => ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);

// A plugin view opened as a window loads this UI too. Its window may only relay events, so it skips everything else.
const label = getCurrentWebviewWindow().label;
if (isViewLabel(label)) {
  const { renderViewWindow } = await import("./view-window");
  renderViewWindow(label);
} else if (await invoke<boolean>("bench_mode")) {
  const { runBench } = await import("./bench");
  await runBench(renderApp);
} else {
  renderApp();
}
