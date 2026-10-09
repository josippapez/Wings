// Stands in for Tauri's bridge so the Wings UI runs in a plain browser. Records every command in
// window.__calls, and keeps the saved workspace in sessionStorage so a reload restores it.
(() => {
  const space = { id: "s1", name: "wings", path: "/tmp/wings", branch: "main" };
  const answers = {
    spaces_list: () => [space], agents_list: () => [], pane_info: () => ({}), git_status: () => ({}),
    plugins_list: () => [], sessions_list: () => [], pane_create: () => "p" + (++window.__panes), panes_list: () => [], bench_mode: () => false,
    workspace_load: () => sessionStorage.getItem("ws"), workspace_save: (a) => void sessionStorage.setItem("ws", a.json),
    "plugin:event|listen": () => 1,
    spaces_add: ({ path }) => ({ id: path, name: path.split("/").at(-1), path, branch: null }),
    // Canned past sessions, filtered like the Rust side: every query word has to match, case aside.
    history_search: ({ query, filter, limit }) => {
      const words = query.toLowerCase().split(/\s+/).filter(Boolean);
      const found = window.__history.filter((s) =>
        (!filter.project || s.cwd === filter.project || s.cwd.startsWith(filter.project + "/")) &&
        (!filter.branch || s.gitBranch === filter.branch) &&
        (!filter.sinceMs || s.lastActiveMs >= filter.sinceMs) &&
        words.every((w) => [s.title, s.firstPrompt, ...s.snippets.map((x) => x.text)].join(" ").toLowerCase().includes(w)));
      const sessions = found.slice(0, limit).map((s) => (words.length ? s : { ...s, snippets: [], matches: 0 }));
      return new Promise((done) => setTimeout(() => done({ sessions, total: found.length, branches: [...new Set(window.__history.map((s) => s.gitBranch))] }), window.__historyDelay ?? 300));
    },
  };
  const now = Date.now();
  const hit = (id, cwd, title, prompt, reply, branch, hoursAgo) => ({
    id: `${id}`.padStart(8, "0") + "-0000-4000-8000-000000000000", cwd, title, firstPrompt: prompt, gitBranch: branch,
    lastActiveMs: now - hoursAgo * 3_600_000, lastActive: null, messages: 12, matches: 2,
    snippets: [{ role: "user", text: prompt }, { role: "assistant", text: reply }],
  });
  window.__history = [
    hit(1, "/tmp/wings", "Session history search", "Add full-text search to the history sheet", "Indexed 1,510 transcripts; search takes 9 ms.", "main", 1),
    hit(2, "/tmp/wings", null, "Fix the login bug in the plugin sheet", "The login button now waits for gh auth.", "fix/login", 30),
    hit(3, "/tmp/wings/.claude/worktrees/agent-1", "Resume with original flags", "Resume Claude panes with their flags", "Restored --model and --permission-mode.", "worktree-agent-1", 50),
    hit(4, "/tmp/shop", "Checkout page totals", "The checkout total is off by the tax", "Tax was added twice; fixed in cart.ts.", "main", 200),
  ];
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
