/** Opening a PR's diff in the viewer, from a pane's badge or from a PR tab, and replies typed in it. */
import { openDiffWhile } from "wings-plugin-sdk/async";
import { git } from "wings-plugin-sdk/git";
import { forgeOf } from "../lib/forges/registry";
import { refOf } from "../lib/providers";
import type { Link } from "../lib/types";
import { diffs, linkFor, status, viewers } from "./state";
import { loadDiff, refreshStatus } from "./status";

/**
 * Opens the viewer at once, in its loading state when the diff isn't cached, then fills it in. `cwd` is the folder
 * of the agent working on the PR, so the viewer's Send to agent offers it first. `focus` is the file and line the viewer
 * scrolls to once the patch is there.
 */
export async function openDiffFor(link: Link, options: { title?: string; cwd?: string; focus?: WingsDiffFocus } = {}) {
  const { cwd, focus } = options;
  const st = status.get(link.url);
  const title = st?.title ?? options.title ?? `Pull request ${refOf(link)}`;
  const subtitle = `${link.repo} ${refOf(link)}`;
  const cached = diffs.get(link.url);
  if (cached && cached.key === st?.key) {
    const { id } = await wings.openDiff({ title, subtitle, cwd, patch: cached.patch, comments: cached.comments, focus });
    return void viewers.set(id, link.url);
  }
  // Not prefetched yet: the viewer opens right away in its loading state, and is filled in.
  await openDiffWhile({ title, subtitle, cwd, focus }, async (id) => {
    viewers.set(id, link.url);
    // A PR from the list or a tab has no pane keeping its status fresh.
    if (!status.get(link.url)) await refreshStatus(link.url);
    const loaded = status.get(link.url);
    if (loaded?.error) throw new Error(loaded.error);
    const { patch, comments } = await loadDiff(link.url);
    return { patch, comments };
  });
}

/** One commit's changes against its first parent, as the forge's commit page shows them, from git in the project's folder. */
export async function openCommit(link: Link, cwd: string, commit: { sha: string; message: string }) {
  if (!/^[0-9a-f]{40}$/.test(commit.sha)) throw new Error("That isn't a commit id.");
  await openDiffWhile({ title: commit.message || commit.sha.slice(0, 7), subtitle: `${link.repo} ${commit.sha.slice(0, 7)}`, cwd }, async () => {
    await git(cwd, ["fetch", "--quiet", "--end-of-options", "origin", commit.sha]);
    return { patch: await git(cwd, ["diff", "--no-color", "--no-ext-diff", `${commit.sha}^`, commit.sha], { raw: true }) };
  });
}

/** A reply typed in a diff viewer is posted to the PR it shows, then added to what it shows. */
export async function replyInViewer({ diffId, replyTo, body }: WingsReply) {
  const url = viewers.get(diffId);
  const link = url && linkFor(url);
  const cached = url ? diffs.get(url) : undefined;
  const root = cached?.comments.find((c) => c.id === replyTo);
  if (!link || !cached || !root?.canReply) throw new Error("This pull request isn't open in a pane any more. Reopen it from the badge.");
  const reply = await forgeOf(link).reply(link, root, body);
  cached.comments = [...cached.comments, reply];
  await wings.updateDiff(diffId, { comments: cached.comments });
}
