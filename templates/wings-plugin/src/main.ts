// Shows each pane's git branch as a badge, and lists the branches in the sidebar. Start here and replace it with
// your own plugin.

import { singleFlight } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";

import { type Branch, ch } from "./lib/messages";

let panes: WingsPane[] = [];
let branches: Branch[] = [];

// The sidebar gets `branches` when it opens, and again after each `changed()`.
const sidebar = ch.share("branches", () => branches);

// Wings sends the panes on every change. A call while `look` runs makes it run once more when it is done.
const refresh = singleFlight(look);

async function look() {
  const found = await Promise.all(
    panes.map(async ({ paneId, cwd, project }) => {
      const branch = await gitOk(cwd, ["branch", "--show-current"]);
      // Wings skips a badge a pane already shows, so every look can set every badge.
      const badge: WingsBadge | null = branch
        ? { label: branch, tone: "info", title: project || "Git branch", rows: [{ label: "Folder", value: cwd }], actions: [{ id: "refresh", label: "Refresh" }] }
        : null;
      await wings.setBadge(paneId, badge).catch(console.warn);
      return branch ? { paneId, project, branch } : null;
    }),
  );
  branches = found.filter((b) => b !== null);
  sidebar.changed();
}

wings.onPanes((list) => {
  panes = list;
  void refresh();
});
// Not while Wings is in the background: a timer that runs git for a badge nobody looks at costs battery.
pollWhileFocused(() => void refresh(), 15_000);

wings.onAction(({ actionId }) => (actionId === "refresh" ? refresh() : undefined));
