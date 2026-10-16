// PR tracker: shows the pull request for each terminal pane's current branch as a pane badge, with live
// status, checks, the diff and review comments. Five forges, each in src/lib/forges/:
// - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
// - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.
// - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
// - Bitbucket Cloud and Codeberg: the REST API with a token saved in Settings.
// When checks fail, the badge says which step failed and why, and Claude can read the logs with `failing_checks`.

import { errorMessage } from "wings-plugin-sdk/async";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { registerTools } from "wings-plugin-sdk/tools";
import { ch } from "./lib/messages";
import { watchMinutes, type WatchMinutes } from "./lib/watch";
import { openDiffFor, replyInViewer } from "./main/diff";
import { refreshLinks } from "./main/links";
import { problemOf } from "./main/problems";
import { pullsFor, refreshProjects, setWatching } from "./main/projects";
import { remoteFor, remotes } from "./main/remote";
import { connectClis, install, retry, signIn } from "./main/retry";
import { links, state, sync } from "./main/state";
import { refreshStatus } from "./main/status";
import { serveTab } from "./main/tab";
import { tools } from "./tools";

const LINK_POLL_MS = 15_000;
const STATUS_POLL_MS = 60_000;
/** A block that opens gets the kept list, read again when it is older than this: a little under the poll, like the list cache. */
const PULLS_STALE_MS = 55_000;
/** A tab's PR older than this is loaded again by the pane poll, so a pane and a tab on it fetch once a minute. */
const PR_STALE_MS = 55_000;

connectClis();

/** Each pane's PR, once a minute. A PR a tab shows is loaded for the tab, whose overview carries the status, so it is fetched once. */
function refreshPanes() {
  const tabs = new Map(ch.watching("pr").map((key) => [key.url, key]));
  for (const url of new Set([...links.values()].map((l) => l.url))) {
    const key = tabs.get(url);
    if (!key) void refreshStatus(url);
    else if (Date.now() - (ch.peek("pr", key)?.at ?? 0) > PR_STALE_MS) ch.refresh("pr", key);
  }
}

wings.onPanes((list) => {
  state.panes = list;
  // A closed pane's PR stops being polled.
  const open = new Set(list.map((p) => p.paneId));
  for (const paneId of [...links.keys()]) {
    if (!open.has(paneId)) {
      links.delete(paneId);
      sync.delete(paneId);
    }
  }
  void refreshLinks();
  // An agent starting or stopping on a PR's branch changes its row.
  void refreshProjects();
});
wings.onProjects(() => void refreshProjects());
// Both polls pause while Wings is in the background, and run once when you come back.
pollWhileFocused(refreshLinks, LINK_POLL_MS);
pollWhileFocused(refreshPanes, STATUS_POLL_MS);
// The project lists are cached for a little under this, so a poll fetches fresh and pane changes in between don't.
pollWhileFocused(() => void refreshProjects(true), STATUS_POLL_MS);

// New pull requests: with an interval set in Settings, the lists are also checked while Wings is in the background.
let watchTimer: ReturnType<typeof setInterval> | undefined;
function watchEvery(minutes: WatchMinutes) {
  setWatching(minutes > 0);
  clearInterval(watchTimer);
  if (minutes) watchTimer = setInterval(() => !wings.focused && void refreshProjects(true), minutes * 60_000);
}
void watchMinutes.get().then(watchEvery);
watchMinutes.onChange(watchEvery);

registerTools(tools);

// ---------- replies ----------

wings.onReply(replyInViewer);

wings.onAction(async ({ paneId, actionId }) => {
  const pane = state.panes.find((p) => p.paneId === paneId);
  const remote = pane && remotes.get(pane.cwd);
  if (actionId === "signin") return signIn(remote);
  if (actionId === "retry") return retry();
  // Wings asks you, installs it in a tab and starts this plugin again once it's there.
  if (actionId === "install") return install(remote);
  const link = links.get(paneId);
  if (!link) return;
  if (actionId === "open") return wings.openUrl(link.url);
  if (actionId === "refresh") return refreshStatus(link.url, { byUser: true });
  if (actionId === "diff") return openDiffFor(link);
});

// ---------- the PR tab ----------

serveTab();

// ---------- the Pull requests block ----------

/** A block on a Project page shows its project's open pull requests, from the same cache as the sidebar section. */
ch.serve(
  "pulls",
  async ({ path }) => {
    try {
      return await pullsFor(path);
    } catch (e) {
      throw new Error(errorMessage(e).slice(0, 300));
    }
  },
  { refetchMs: STATUS_POLL_MS, staleMs: PULLS_STALE_MS },
);

/** The block's button: what the pane badge's Sign in, Add token or Install does for this project's host. */
ch.answer("pulls-action", async ({ path }) => {
  const remote = await remoteFor(path);
  // A host no CLI knew: look at the CLIs' hosts again.
  if (remote.kind === "none") await retry();
  else if (problemOf(remote)?.kind === "missing") await install(remote);
  else await signIn(remote);
  ch.refresh("pulls", { path });
});

// Settings saved or removed a token: ask again with it.
ch.on("tokens-changed", () => void retry().then(() => refreshProjects(true)));
