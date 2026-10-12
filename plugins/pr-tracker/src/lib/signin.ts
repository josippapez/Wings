/** Signing in to a provider's CLI from the badge, and trying again from scratch. */
import { refreshLinks } from "./links";
import { lookups, PROVIDERS, problems, signingIn, status } from "./state";
import type { GitlabRemote, Remote } from "./types";
import { render } from "./view";

const SIGN_IN_TIMEOUT_MS = 300_000;

/**
 * Runs the provider's own login so you never need a terminal. `az` and `glab` open the browser themselves.
 * `gh` doesn't without a terminal: it prints a one-time code and a URL, so the badge shows the code and
 * Wings opens the URL.
 */
export async function signIn(remote: Remote | undefined) {
  const provider = remote?.kind;
  if (!PROVIDERS[provider as string]) return;
  if (signingIn.has(provider as string)) return;
  signingIn.set(provider as string, {});
  render();
  try {
    const out =
      provider === "github"
        ? await wings.exec("gh", ["auth", "login", "--web", "--clipboard", "--hostname", "github.com"], {
            timeoutMs: SIGN_IN_TIMEOUT_MS,
            onOutput: (line) => {
              const code = line.match(/code(?::| \()\s*([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1];
              if (code) {
                signingIn.set(provider, { code });
                render();
              }
              const url = line.match(/https:\/\/github\.com\/\S+/)?.[0];
              if (url) void wings.openUrl(url);
            },
          })
        : provider === "gitlab"
          ? await wings.exec("glab", ["auth", "login", "--web", "--hostname", (remote as GitlabRemote).host], { timeoutMs: SIGN_IN_TIMEOUT_MS })
          : await wings.exec("az", ["login", "--allow-no-subscriptions", "--output", "none"], { timeoutMs: SIGN_IN_TIMEOUT_MS });
    if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "Sign-in failed");
  } finally {
    signingIn.delete(provider as string);
  }
  return retry();
}

export function retry() {
  problems.clear();
  lookups.clear();
  status.clear();
  return refreshLinks();
}
