/** Opening a PR's diff in the viewer, from a pane's badge or from a PR tab. */
import { diffs, refOf, status, viewers } from "./state";
import { loadDiff, refreshStatus } from "./status";
import type { Link } from "./types";

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
  // Not prefetched yet: open the viewer right away in its loading state, then fill it in.
  const { id } = await wings.openDiff({ title, subtitle, cwd, focus });
  viewers.set(id, link.url);
  try {
    // A PR from the list or a tab has no pane keeping its status fresh.
    if (!status.get(link.url)) await refreshStatus(link.url);
    const loaded = status.get(link.url);
    if (loaded?.error) throw new Error(loaded.error);
    const { patch, comments } = await loadDiff(link.url);
    await wings.updateDiff(id, { patch, comments });
  } catch (error) {
    await wings.updateDiff(id, { error: String((error as Error).message ?? error) });
  }
}
