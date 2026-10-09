import { invoke } from "@tauri-apps/api/core";
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

if (await invoke<boolean>("bench_mode")) {
  const { runBench } = await import("./bench");
  await runBench();
} else {
  // No StrictMode: its double-mount in dev would spawn every terminal's shell twice.
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
}
