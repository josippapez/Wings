// ---------- finding the PR for a pane ----------
import { cached } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";
import { forgeFor, isKnown } from "../lib/forges/registry";
import type { KnownRemote } from "../lib/types";
import { render } from "./badges";
import { answered, explains } from "./problems";
import { remoteFor } from "./remote";
import { links, state, status, sync } from "./state";
import { refreshStatus } from "./status";

const LOOKUP_TTL_MS = 60_000;

/** `repo branch` → the PR the branch has, so panes on the same branch share one network lookup. A failed one isn't kept, so the next pane asks again. */
export const lookups = cached(
  async ({ cwd, remote, branch }: { cwd: string; remote: KnownRemote; branch: string }) => {
    try {
      const link = await forgeFor(remote).find(cwd, remote, branch);
      answered(remote);
      return link;
    } catch (error) {
      // The account can't say, and the pane's badge says why: there is no PR to show for now.
      if (explains(remote, error)) return null;
      throw error;
    }
  },
  { ttlMs: LOOKUP_TTL_MS, key: ({ remote, branch }) => `${forgeFor(remote).repoKey(remote)} ${branch}` },
);

async function findForPane(pane: WingsPane) {
  const remote = await remoteFor(pane.cwd);
  if (!isKnown(remote)) return null;
  // A Claude pane's PR is the one Claude Code recorded, for a forge whose links come from there.
  if (pane.session) {
    const link = await forgeFor(remote).fromSession?.(pane.session.sessionId);
    if (link) return link;
  }
  const branch = await gitOk(pane.cwd, ["branch", "--show-current"]);
  if (!branch) return null;
  return lookups.get({ cwd: pane.cwd, remote, branch });
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
        const counts = (await gitOk(pane.cwd, ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])) ?? "0 0";
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
