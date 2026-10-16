/** Signing in to a provider's CLI from the badge, installing it, and trying again from scratch. */
import { forgeFor, forges, isKnown } from "../lib/forges/registry";
import { ch } from "../lib/messages";
import type { Remote } from "../lib/types";
import { render } from "./badges";
import { lookups, refreshLinks } from "./links";
import { codes, forgetProblems } from "./problems";
import { forgetLists } from "./projects";
import { forgetHosts } from "./remote";
import { diffs, failures, status } from "./state";

const clis = Object.values(forges).flatMap((forge) => (forge.login ? [forge.login.cli] : []));

/** Forgets what the tracker kept about its accounts and their pull requests, so the next look at them asks again. */
function clearCaches() {
  forgetHosts();
  forgetLists();
  forgetProblems();
  lookups.clear();
  status.clear();
  diffs.clear();
  failures.clear();
  for (const forge of Object.values(forges)) forge.forget?.();
}

/** The badges follow each CLI's sign-in and problems, and whichever CLI is tried again or signed in to, the caches start over. */
export function connectClis() {
  for (const cli of clis) {
    cli.onChange(render);
    cli.onRetry(clearCaches);
  }
}

/**
 * Runs the provider's own login so you never need a terminal. `az` and `glab` open the browser themselves.
 * `gh` doesn't without a terminal: it prints a one-time code and a URL, so the badge shows the code and
 * Wings opens the URL.
 */
export async function signIn(remote: Remote | undefined) {
  if (!remote || !isKnown(remote)) return;
  const forge = forgeFor(remote);
  // Bitbucket and Codeberg have no CLI to sign in with: the token is pasted in Settings.
  if (!forge.login) return wings.openSettings();
  const key = forge.problemKey(remote);
  try {
    await forge.login.signIn(remote, (code) => {
      codes.set(key, code);
      render();
    });
  } finally {
    codes.delete(key);
  }
  await refreshLinks();
  refreshTabs();
}

/** Asks Wings to install the CLI a remote's forge works through. */
export function install(remote: Remote | undefined) {
  return remote && isKnown(remote) ? forgeFor(remote).login?.cli.install() : undefined;
}

export async function retry() {
  await Promise.all(clis.map((cli) => cli.retry()));
  await refreshLinks();
  refreshTabs();
}

/** The open tabs' PRs load again, after a sign-in or Try again. */
function refreshTabs() {
  ch.refresh("pr");
  ch.refresh("pr-comments");
  ch.refresh("pr-pushes");
}
