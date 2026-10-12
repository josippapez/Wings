// Shows what changed in each pane's git repo: a badge with the counts, a sidebar with the files as a tree, and
// the diff viewer for one file or all of them. Everything is local git, about 25 ms a call, cached per repo.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { refreshBadges } from "./lib/badge";
import { openChanges } from "./lib/diff";
import { basename, defaults, rootOf, roots } from "./lib/git";
import { openCommit } from "./lib/history";
import { panes, setPanes } from "./lib/panes";
import { historyTab, POLL_MS, sendHistory, sendSidebar, sidebar, sidebarOpen, sidebarRoot } from "./lib/sidebar";
import { changedFiles, recentCommits } from "./lib/tools";
import type { GraphRepoMessage } from "./lib/types";

/** The repo the graph view shows. */
let graphRoot: string | null = null;

wings.onBroadcast((message) => {
  if (!message || typeof message !== "object") return;
  const m = message as Record<string, unknown>;
  if (m.type === "hello" || m.type === "refresh" || m.type === "mode" || m.type === "history") {
    sidebar.seen = Date.now();
    sidebar.tab = m.tab === "history" || m.type === "history" ? "history" : "changes";
    if (m.type === "mode" && (m.mode === "uncommitted" || m.mode === "branch")) sidebar.mode = m.mode;
    if (m.type === "refresh" && m.byUser) defaults.clear();
    if (m.type === "history") {
      historyTab.query = typeof m.query === "string" ? m.query.trim().slice(0, 200) : "";
      historyTab.all = m.all === true;
    }
    if (sidebar.tab === "history") {
      const skip = m.type === "history" && Number.isInteger(m.skip) ? Math.max(0, m.skip as number) : 0;
      void sendHistory(skip, { poll: m.type === "refresh" && !m.byUser }).catch(console.error);
    } else void sendSidebar({ fresh: m.type === "refresh" }).catch(console.error);
  }
  if (m.type === "open" && typeof m.root === "string" && (m.mode === "uncommitted" || m.mode === "branch")) {
    void openChanges(m.root, m.mode, typeof m.path === "string" ? m.path : null).catch(console.error);
  }
  if (m.type === "commit" && typeof m.root === "string" && typeof m.hash === "string" && /^[0-9a-f]{40}$/.test(m.hash)) {
    const { hash, subject, author, at } = m;
    void openCommit(m.root, { hash, subject: typeof subject === "string" ? subject : "", author: typeof author === "string" ? author : "", at: typeof at === "number" ? at : 0 }).catch(console.error);
  }
  if (m.type === "graph") {
    graphRoot = typeof m.root === "string" ? m.root : sidebar.root;
    void wings.openView("graph").catch(console.error);
    void wings.broadcast({ type: "graph-repo", root: graphRoot, name: graphRoot && basename(graphRoot) } satisfies GraphRepoMessage).catch(() => {});
  }
  if (m.type === "graph-hello") {
    void (async () => {
      const root = graphRoot ?? (await sidebarRoot());
      await wings.broadcast({ type: "graph-repo", root, name: root && basename(root) } satisfies GraphRepoMessage);
    })().catch(console.error);
  }
});

// ---------- panes, actions and the tool ----------

wings.onPanes((list) => {
  const before = new Set(panes.map((p) => `${p.paneId}\0${p.cwd}`));
  setPanes(list);
  if (list.some((p) => !before.has(`${p.paneId}\0${p.cwd}`)) || list.length !== before.size) void refreshBadges().catch(console.error);
  if (!sidebarOpen()) return;
  void sidebarRoot().then((root) => (root === sidebar.root ? undefined : sidebar.tab === "history" ? sendHistory(0) : sendSidebar())).catch(console.error);
});

// Paused while Wings is in the background, and runs once when you come back.
pollWhileFocused(() => {
  // A repo becomes one after `git init`, and stops after it's removed.
  for (const [cwd, root] of roots) void root.then((r) => r === null && roots.delete(cwd));
  void refreshBadges().catch(console.error);
}, POLL_MS);

wings.onAction(async ({ paneId, actionId }) => {
  const pane = panes.find((p) => p.paneId === paneId);
  const root = pane && (await rootOf(pane.cwd));
  if (!root) return;
  if (actionId === "open") return openChanges(root, "uncommitted", null);
  if (actionId === "branch") return openChanges(root, "branch", null);
  if (actionId === "refresh") return refreshBadges({ fresh: true });
  if (actionId === "sidebar") return wings.openView("changes");
});

wings.onTool("recent_commits", recentCommits);

wings.onTool("changed_files", changedFiles);
