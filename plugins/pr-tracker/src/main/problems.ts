/**
 * What is wrong with a pane's host. A CLI that is missing or signed out is kept by the CLI helper; a token that is missing
 * or refused, and a CLI that can't list the PRs, are kept here, since the helper has nothing for them.
 */
import { CliError } from "wings-plugin-sdk/cli";
import { forgeFor, isKnown } from "../lib/forges/registry";
import { AccountProblem, type Forge, type Login } from "../lib/forges/types";
import type { Problem as Shown } from "../lib/messages";
import type { KnownRemote, Remote } from "../lib/types";

export type Problem = { remote: KnownRemote; forge: Forge; message: string } & (
  | { kind: "token" }
  | { kind: "unavailable" }
  /** A CLI that is missing or signed out: `login` is how the pane offers to install it or sign in. */
  | { kind: "missing" | "signed-out"; login: Login }
);

/** Problem key → what a forge's own code found wrong. */
const own = new Map<string, { kind: "token" | "unavailable"; message: string }>();
/** Problem key → the one-time code of the sign-in running for it, which the badge shows. */
export const codes = new Map<string, string>();

/** What is wrong with the host of a remote, or null. */
export function problemOf(remote: Remote | undefined): Problem | null {
  if (!remote || !isKnown(remote)) return null;
  const forge = forgeFor(remote);
  const key = forge.problemKey(remote);
  const { login } = forge;
  const found = login?.cli.problem(key);
  if (login && found) return { remote, forge, login, kind: found.kind, message: login.words(remote)[found.kind === "missing" ? "missing" : "signedOut"] };
  const mine = own.get(key);
  return mine ? { remote, forge, ...mine } : null;
}

/**
 * Notes what an error from a host says about its account, and tells whether it was that, which leaves a pane with no PR
 * to show and its badge saying why, rather than a failure.
 */
export function explains(remote: KnownRemote, error: unknown): boolean {
  if (error instanceof AccountProblem) {
    own.set(forgeFor(remote).problemKey(remote), { kind: error.kind, message: error.message });
    return true;
  }
  // The CLI helper has kept a CLI that is missing or signed out.
  return error instanceof CliError && error.kind !== "failed";
}

/** The host answered, so whatever its token or CLI had wrong is over. */
export function answered(remote: KnownRemote) {
  own.delete(forgeFor(remote).problemKey(remote));
}

const ACTIONS = { missing: "Install", "signed-out": "Sign in", token: "Add token" } as const;

/** What a block or a tab shows for a host that needs signing in, a token or a CLI: a short message and its button. Null for anything else. */
export function shownProblem(remote: Remote | undefined): Shown | null {
  const problem = problemOf(remote);
  return problem && problem.kind !== "unavailable" ? { message: problem.message, action: ACTIONS[problem.kind] } : null;
}

export function forgetProblems() {
  own.clear();
  codes.clear();
}
