// Stands in for Tauri's bridge so the real Wings UI runs in a bare WKWebView, with three plugins that badge every pane.
(() => {
  const space = { id: "s1", name: "wings", path: "/tmp/wings", branch: "main" };
  const plugin = (id) => ({
    id, name: id, version: "1.0.0", description: "", main: "plugin.js", iconSvg: null, enabled: true, approved: true, source: null, dev: true,
    permissions: { exec: [], transcript: [], openUrl: [], fetch: [], post: [], panes: null, notify: false, statusline: false },
    contributes: { ui: ["badges"], mcpTools: [], panels: [], sidebars: [], views: [] },
  });
  let panes = 0;
  const answers = {
    spaces_list: () => [space], agents_list: () => [], pane_info: () => ({}), git_status: () => ({}), sessions_list: () => [],
    plugins_list: () => ["changes", "pr", "context"].map(plugin), pane_create: () => "p" + ++panes, panes_list: () => [],
    bench_mode: () => false, workspace_load: () => null, "plugin:event|listen": () => 1,
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (cb) => { const id = (Math.random() * 1e9) | 0; window[`_${id}`] = cb; return id; },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => answers[cmd]?.(args) ?? null,
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
