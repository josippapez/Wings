// Stands in for Tauri's bridge so the Wings UI runs in a plain browser. Records every command in
// window.__calls, and keeps the saved workspace in sessionStorage so a reload restores it.
(() => {
  const space = { id: "s1", name: "wings", path: "/tmp/wings", branch: "main" };
  const answers = {
    spaces_list: () => [space], agents_list: () => [], pane_info: () => ({}), git_status: () => ({}),
    plugins_list: () => [], sessions_list: () => [], pane_create: () => "p" + (++window.__panes), bench_mode: () => false,
    workspace_load: () => sessionStorage.getItem("ws"), workspace_save: (a) => void sessionStorage.setItem("ws", a.json),
    "plugin:event|listen": () => 1,
  };
  window.__panes = 0;
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (cb) => { const id = (Math.random() * 1e9) | 0; window[`_${id}`] = cb; return id; },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => { window.__calls.push({ cmd, args, t: performance.now() }); return answers[cmd]?.(args) ?? null; },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
