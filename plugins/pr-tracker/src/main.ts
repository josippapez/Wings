// PR tracker: shows the pull request for each terminal pane's current branch as a pane badge, with live
// status, checks, the diff and review comments. Five forges, each in src/lib/forges/:
// - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
// - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.
// - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
// - Bitbucket Cloud and Codeberg: the REST API with a token saved in Settings.
// When checks fail, the badge says which step failed and why, and Claude can read the logs with `failing_checks`.

import { errorMessage } from "wings-plugin-sdk/async";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { linkOf } from "./lib/forges/registry";
import type { PullsAction, PullsChanged, PullsRequest, PullsResponse } from "./lib/pulls";
import type { Link, PrArgs } from "./lib/types";
import { WATCH_KEY, watchOf } from "./lib/watch";
import { openDiffFor, replyInViewer } from "./main/diff";
import { failingChecks } from "./main/failing";
import { refreshLinks } from "./main/links";
import { problemOf } from "./main/problems";
import { markSeen, pullsFor, refreshProjects, setWatching } from "./main/projects";
import { remoteFor, remotes } from "./main/remote";
import { connectClis, install, retry, signIn } from "./main/retry";
import { known, links, state, sync } from "./main/state";
import { refreshStatus } from "./main/status";

const LINK_POLL_MS = 15_000;
const STATUS_POLL_MS = 60_000;

connectClis();

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
pollWhileFocused(() => [...new Set([...links.values()].map((l) => l.url))].forEach((url) => refreshStatus(url)), STATUS_POLL_MS);
// The project lists are cached for a little under this, so a poll fetches fresh and pane changes in between don't.
pollWhileFocused(() => void refreshProjects(true), STATUS_POLL_MS);

// New pull requests: with an interval set in Settings, the lists are also checked while Wings is in the background.
let watchTimer: ReturnType<typeof setInterval> | undefined;
async function loadWatch() {
  const minutes = watchOf(await wings.storage.get(WATCH_KEY));
  setWatching(minutes > 0);
  clearInterval(watchTimer);
  if (minutes) watchTimer = setInterval(() => !wings.focused && void refreshProjects(true), minutes * 60_000);
}
void loadWatch();

wings.onTool("failing_checks", (_input, { paneId }) => failingChecks(paneId));

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

/** What a "pr" tab asks of this script, since only it can open the diff viewer that takes replies and knows the agent panes. */
type PrRequest = { type: "pr-request"; rid: string; op: "diff" | "agent" | "seen"; args: PrArgs; focus?: WingsDiffFocus };

async function handleRequest({ op, args, focus }: PrRequest) {
  if (op === "seen") return markSeen(args.url);
  let link: Link | undefined = [...links.values()].find((l) => l.url === args.url);
  if (!link) {
    // No pane is on it: diff in the folder of the project whose row opened it, not the first project that listed the repo.
    link = known.get(args.url) ?? (await linkOf(args));
    if ("cwd" in link) link = { ...link, cwd: args.cwd };
    known.set(args.url, link);
  }
  if (op === "diff") return openDiffFor(link, { title: args.title, focus });
  // The agent working on this PR's branch: Send to agent in the viewer offers the one in this folder first.
  const agent = state.panes.find((p) => p.session && links.get(p.paneId)?.url === args.url);
  if (!agent) throw new Error("No agent is working on this branch. Start Claude in a terminal on it, then try again.");
  return openDiffFor(link, { title: args.title, cwd: agent.cwd, focus });
}

wings.onBroadcast(async (message) => {
  const request = message as PrRequest | null;
  if (request?.type !== "pr-request") return;
  let error: string | undefined;
  try {
    await handleRequest(request);
  } catch (e) {
    error = errorMessage(e);
  }
  await wings.broadcast({ type: "pr-response", rid: request.rid, error });
});

// ---------- the Pull requests block ----------

/** A block on a Project page asks for its project's open pull requests, from the same cache as the sidebar section. */
wings.onBroadcast(async (message) => {
  const request = message as PullsRequest | null;
  if (request?.type !== "pulls-request" || typeof request.path !== "string") return;
  let response: Omit<PullsResponse, "type" | "rid">;
  try {
    response = await pullsFor(request.path);
  } catch (e) {
    response = { error: errorMessage(e).slice(0, 300) };
  }
  await wings.broadcast({ type: "pulls-response", rid: request.rid, ...response } satisfies PullsResponse);
});

/** The block's button: what the pane badge's Sign in, Add token or Install does for this project's host. */
wings.onBroadcast(async (message) => {
  const request = message as PullsAction | null;
  if (request?.type !== "pulls-action" || typeof request.path !== "string") return;
  const remote = await remoteFor(request.path);
  // A host no CLI knew: look at the CLIs' hosts again.
  if (remote.kind === "none") await retry();
  else if (problemOf(remote)?.kind === "missing") await install(remote);
  else await signIn(remote);
  await wings.broadcast({ type: "pulls-changed" } satisfies PullsChanged);
});

// Settings saved a token: ask again with it. Or it changed how often to check for new PRs.
wings.onBroadcast((message) => {
  if ((message as { settingsChanged?: boolean } | null)?.settingsChanged) void retry().then(() => refreshProjects(true));
  if ((message as { watchChanged?: boolean } | null)?.watchChanged) void loadWatch();
});
