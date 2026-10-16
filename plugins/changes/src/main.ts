// Shows what changed in each pane's git repo: a badge with the counts, a sidebar with the files as a tree, and
// the diff viewer for one file or all of them. Everything is local git, about 25 ms a call, cached per repo.

import { repoRoot } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { registerTools } from "wings-plugin-sdk/tools";

import { forgetRepos } from "./lib/changes";
import { openChanges } from "./lib/diff";
import { POLL_MS } from "./lib/messages";
import { refreshBadges } from "./main/badges";
import { serveSidebar } from "./main/sidebar";
import { tools } from "./tools";

let panes: WingsPane[] = [];
const sidebar = serveSidebar(() => panes);

wings.onPanes((list) => {
  const before = new Set(panes.map((p) => `${p.paneId}\0${p.cwd}`));
  panes = list;
  if (list.some((p) => !before.has(`${p.paneId}\0${p.cwd}`)) || list.length !== before.size) void refreshBadges(panes);
  void sidebar.panesChanged();
});

// Paused while Wings is in the background, and runs once when you come back.
pollWhileFocused(() => void refreshBadges(panes), POLL_MS);

wings.onAction(async ({ paneId, actionId }) => {
  const pane = panes.find((p) => p.paneId === paneId);
  if (actionId === "refresh") forgetRepos();
  const root = pane && (await repoRoot(pane.cwd));
  if (!root) return;
  if (actionId === "open") return openChanges(root, "uncommitted", null);
  if (actionId === "branch") return openChanges(root, "branch", null);
  if (actionId === "refresh") return refreshBadges(panes, { fresh: true });
  if (actionId === "sidebar") return wings.openView("changes");
});

registerTools(tools);
