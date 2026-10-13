// PR tracker: shows the pull request for each terminal pane's current branch as a pane badge, with live
// status, checks, the diff and review comments. Three providers:
// - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
// - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
// - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.
// When checks fail, the badge says which step failed and why, and Claude can read the logs with `failing_checks`.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { azureReply } from "./lib/azure";
import { openDiffFor } from "./lib/diffview";
import { githubReply } from "./lib/github";
import { gitlabReply } from "./lib/gitlab";
import { refreshLinks } from "./lib/links";
import { markSeen, refreshProjects } from "./lib/projects";
import { linkOf } from "./lib/refs";
import { retry, signIn } from "./lib/signin";
import { diffs, known, links, linkFor, problems, remotes, state, sync, viewers } from "./lib/state";
import { refreshStatus } from "./lib/status";
import { failingChecks } from "./lib/tool";
import type { Comment, Diff, Link, PrArgs } from "./lib/types";
import { problemKey } from "./lib/view";

const LINK_POLL_MS = 15_000;
const STATUS_POLL_MS = 60_000;

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

wings.onTool("failing_checks", failingChecks);

// ---------- replies ----------

wings.onReply(async ({ diffId, replyTo, body }) => {
  const url = viewers.get(diffId);
  const link = url && linkFor(url);
  const cached = (url && diffs.get(url)) as Diff | undefined;
  const root = cached?.comments.find((c) => c.id === replyTo);
  if (!link || !root?.canReply) throw new Error("This pull request isn't open in a pane any more. Reopen it from the badge.");
  const reply = await ({ github: githubReply, azure: azureReply, gitlab: gitlabReply }[link.provider] as (link: Link, root: Comment, body: string) => Promise<Comment>)(
    link,
    root,
    body,
  );
  cached!.comments = [...cached!.comments, reply];
  await wings.updateDiff(diffId, { comments: cached!.comments });
});

wings.onAction(async ({ paneId, actionId }) => {
  const pane = state.panes.find((p) => p.paneId === paneId);
  if (actionId === "signin") return signIn(pane && remotes.get(pane.cwd));
  if (actionId === "retry") return retry();
  // Wings asks you, installs it in a tab and starts this plugin again once it's there.
  if (actionId === "install") {
    const program = pane && problems.get(problemKey(remotes.get(pane.cwd)))?.missing;
    return program && wings.installTool(program);
  }
  const link = links.get(paneId);
  if (!link) return;
  if (actionId === "open") return wings.openUrl(link.url);
  if (actionId === "refresh") return refreshStatus(link.url, { byUser: true });
  if (actionId === "diff") return openDiffFor(link);
});

// ---------- the PR tab ----------

/** What a "pr" tab asks of this script, since only it can open the diff viewer that takes replies and knows the agent panes. */
type PrRequest = { type: "pr-request"; rid: string; op: "diff" | "agent" | "seen"; args: PrArgs };

async function handleRequest({ op, args }: PrRequest) {
  if (op === "seen") return markSeen(args.url);
  let link: Link | undefined = [...links.values()].find((l) => l.url === args.url);
  if (!link) {
    // No pane is on it: diff in the folder of the project whose row opened it, not the first project that listed the repo.
    link = known.get(args.url) ?? (await linkOf(args));
    if (link.provider === "azure") link = { ...link, cwd: args.cwd };
    known.set(args.url, link);
  }
  if (op === "diff") return openDiffFor(link, { title: args.title });
  // The agent working on this PR's branch: Send to agent in the viewer offers the one in this folder first.
  const agent = state.panes.find((p) => p.session && links.get(p.paneId)?.url === args.url);
  if (!agent) throw new Error("No agent is working on this branch. Start Claude in a terminal on it, then try again.");
  return openDiffFor(link, { title: args.title, cwd: agent.cwd });
}

wings.onBroadcast(async (message) => {
  const request = message as PrRequest | null;
  if (request?.type !== "pr-request") return;
  let error: string | undefined;
  try {
    await handleRequest(request);
  } catch (e) {
    error = String((e as Error).message ?? e);
  }
  await wings.broadcast({ type: "pr-response", rid: request.rid, error });
});
