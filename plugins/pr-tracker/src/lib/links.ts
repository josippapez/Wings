// ---------- finding the PR for a pane ----------
import { findAzure } from "./azure";
import { git } from "./cli";
import { findGithub, fromTranscript } from "./github";
import { findGitlab } from "./gitlab";
import { remoteFor } from "./remote";
import { links, lookups, state, status, sync } from "./state";
import { refreshStatus } from "./status";
import type { Link, Remote } from "./types";
import { render } from "./view";

const LOOKUP_TTL_MS = 60_000;

async function findForPane(pane: WingsPane) {
  const remote = await remoteFor(pane.cwd);
  if (remote.kind === "none") return null;
  if (remote.kind === "github" && pane.session) {
    const link = await fromTranscript(pane.session.sessionId);
    if (link) return link;
  }
  const branch = await git(pane.cwd, ["branch", "--show-current"]).catch(() => "");
  if (!branch) return null;
  const key = `${remote.origin} ${branch}`;
  const hit = lookups.get(key);
  if (hit && Date.now() - hit.at < LOOKUP_TTL_MS) return hit.link;
  const find = { github: findGithub, azure: findAzure, gitlab: findGitlab }[remote.kind] as (cwd: string, remote: Remote, branch: string) => Promise<Link | null>;
  const link = find(pane.cwd, remote, branch);
  lookups.set(key, { at: Date.now(), link });
  link.catch(() => lookups.delete(key));
  return link;
}

export async function refreshLinks() {
  await Promise.all(
    state.panes.map(async (pane) => {
      try {
        const link = await findForPane(pane);
        if (!link) return void links.delete(pane.paneId);
        const known = links.get(pane.paneId);
        links.set(pane.paneId, link);
        // Fails without an upstream branch, which means nothing to push or pull yet.
        const counts = await git(pane.cwd, ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).catch(() => "0 0");
        const [behind, ahead] = counts.split(/\s+/).map(Number);
        sync.set(pane.paneId, { ahead, behind });
        if (known?.url !== link.url && !status.has(link.url)) void refreshStatus(link.url);
      } catch (error) {
        console.error(error);
      }
    }),
  );
  render();
}
