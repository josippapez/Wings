// ---------- finding the PR for a pane ----------
import { cached } from "wings-plugin-sdk/async";
import { gitOk, watchedProject } from "wings-plugin-sdk/git";
import { forgeFor, isKnown } from "../lib/forges/registry";
import type { KnownRemote, Link } from "../lib/types";
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

/** paneId → the project whose repo the pane is in, or null when no project has it, so Wings sends no change for it. */
const watched = new Map<string, string | null>();
/** paneId → the branch read last, so the 60 s lookup runs no git. */
const branches = new Map<string, string | null>();
/** sessionId → the PR Claude Code recorded for it: read once, then kept up to date from `onTranscript`. */
const sessionLinks = new Map<string, Link | null>();

/** The project whose repo the pane is in, or null when no project has it: only those panes get a poll for their branch. */
export const projectOf = (pane: WingsPane) => watched.get(pane.paneId) ?? null;

/** What is kept per pane goes when the pane closes. */
export function forgetPanes(open: Set<string>) {
  for (const map of [watched, branches]) for (const paneId of map.keys()) if (!open.has(paneId)) map.delete(paneId);
}

/** Claude Code recorded a PR for a session. */
export function sessionLinked(sessionId: string, link: Link) {
  sessionLinks.set(sessionId, link);
}

async function findForPane(pane: WingsPane, local: boolean) {
  if (local) watched.set(pane.paneId, await watchedProject(pane.cwd, await wings.projects()));
  const remote = await remoteFor(pane.cwd);
  if (!isKnown(remote)) return null;
  // A Claude pane's PR is the one Claude Code recorded, for a forge whose links come from there.
  const forge = forgeFor(remote);
  // Only Claude Code records one: an opencode session has no transcript. Wings before 0.1.60 sends no kind.
  if (pane.session && (pane.session.kind ?? "claude") === "claude" && forge.fromSession) {
    const { sessionId } = pane.session;
    if (!sessionLinks.has(sessionId)) sessionLinks.set(sessionId, await forge.fromSession(sessionId));
    const link = sessionLinks.get(sessionId);
    if (link) return link;
  }
  if (local || !branches.has(pane.paneId)) branches.set(pane.paneId, await gitOk(pane.cwd, ["branch", "--show-current"]));
  const branch = branches.get(pane.paneId);
  if (!branch) return null;
  return lookups.get({ cwd: pane.cwd, remote, branch });
}

/**
 * Finds the PR of the panes `which` picks, all of them when it is left out. `local` reads each pane's branch and its
 * commits to push and pull with git: a repo change, a new pane and the poll of a repo no project has do that. Without it
 * the branch read last is looked up again, which is the 60 s cycle's work and runs no git.
 */
export async function refreshLinks({ local = true, which }: { local?: boolean; which?: (pane: WingsPane) => boolean } = {}) {
  await Promise.all(
    state.panes
      .filter((pane) => !which || which(pane))
      .map(async (pane) => {
        try {
          const link = await findForPane(pane, local);
          if (!link) return void links.delete(pane.paneId);
          const known = links.get(pane.paneId);
          links.set(pane.paneId, link);
          if (local) {
            // Fails without an upstream branch, which means nothing to push or pull yet.
            const counts = (await gitOk(pane.cwd, ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])) ?? "0 0";
            const [behind, ahead] = counts.split(/\s+/).map(Number);
            sync.set(pane.paneId, { ahead, behind });
          }
          if (known?.url !== link.url && !status.has(link.url)) void refreshStatus(link.url);
        } catch (error) {
          console.error(error);
        }
      }),
  );
  render();
}
