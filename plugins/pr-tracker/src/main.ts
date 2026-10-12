// PR tracker: shows the pull request for each terminal pane's current branch as a pane badge, with live
// status, checks, the diff and review comments. Three providers:
// - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
// - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
// - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.
// When checks fail, the badge says which step failed and why, and Claude can read the logs with `failing_checks`.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { azureReply } from "./lib/azure";
import { githubReply } from "./lib/github";
import { gitlabReply } from "./lib/gitlab";
import { refreshLinks } from "./lib/links";
import { retry, signIn } from "./lib/signin";
import { diffs, links, linkFor, problems, refOf, remotes, state, status, sync, viewers } from "./lib/state";
import { loadDiff, refreshStatus } from "./lib/status";
import { failingChecks } from "./lib/tool";
import type { Comment, Diff, Link } from "./lib/types";
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
});
// Both polls pause while Wings is in the background, and run once when you come back.
pollWhileFocused(refreshLinks, LINK_POLL_MS);
pollWhileFocused(() => [...new Set([...links.values()].map((l) => l.url))].forEach((url) => refreshStatus(url)), STATUS_POLL_MS);

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
  if (actionId === "diff") {
    const st = status.get(link.url);
    const title = st?.title ?? `Pull request ${refOf(link)}`;
    const subtitle = `${link.repo} ${refOf(link)}`;
    const cached = diffs.get(link.url);
    if (cached && cached.key === st?.key) {
      const { id } = await wings.openDiff({ title, subtitle, patch: cached.patch, comments: cached.comments });
      return void viewers.set(id, link.url);
    }
    // Not prefetched yet: open the viewer right away in its loading state, then fill it in.
    const { id } = await wings.openDiff({ title, subtitle });
    viewers.set(id, link.url);
    try {
      const { patch, comments } = await loadDiff(link.url);
      await wings.updateDiff(id, { patch, comments });
    } catch (error) {
      await wings.updateDiff(id, { error: String((error as Error).message ?? error) });
    }
  }
});
