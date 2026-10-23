// A linked pull request's state, read with the host's CLI: how to ask for it, what the answer means, and what that does to
// an issue that is still in progress or in review.

import { isLive } from "./journal";
import { childrenOf } from "./relations";
import type { Issue, Op } from "./domain";

export type PrState = "merged" | "closed" | "open";

/** The program and arguments that print a PR's state, or null for a link that isn't a GitHub or Azure DevOps PR. */
export function prStateCommand(url: string): { program: "gh" | "az"; args: string[] } | null {
  if (/^https:\/\/github\.com\/[^/]+\/[^/]+\/pull\/\d+$/.test(url)) return { program: "gh", args: ["pr", "view", url, "--json", "state", "--jq", ".state"] };
  const ado = url.match(/^https:\/\/dev\.azure\.com\/([^/]+)\/[^/]+\/_git\/[^/]+\/pullrequest\/(\d+)$/);
  if (ado) return { program: "az", args: ["repos", "pr", "show", "--id", ado[2], "--organization", `https://dev.azure.com/${ado[1]}`, "--query", "status", "--output", "tsv"] };
  return null;
}

/** What the CLI printed: gh says MERGED, CLOSED or OPEN, az says completed, abandoned or active. */
export function prStateOf(output: string): PrState | null {
  const word = output.trim().toLowerCase();
  if (word === "merged" || word === "completed") return "merged";
  if (word === "closed" || word === "abandoned") return "closed";
  if (word === "open" || word === "active") return "open";
  return null;
}

/** The note Issues leaves on a live issue whose PR is closed without merging. It's left once, and the issue stays as it is. */
export const closedNote = (url: string) => `The linked pull request was closed without merging: ${url}. Still open here, so check whether the work is done.`;
/** The note on a merged PR's issue that still has open child issues, which stays open with them. Left once. */
export const childrenNote = (url: string) => `The linked pull request was merged: ${url}. Its child issues are still open, so it stays open too.`;

const noteOnce = (issue: Issue, text: string): Op[] => (issue.log.some((e) => e.kind === "note" && e.text === text) ? [] : [{ kind: "note", key: issue.key, text }]);

/**
 * What a PR's state does to an issue linked to it, among the project's `issues`: a merged PR finishes a live issue unless
 * a child issue is still open, a closed one leaves a note once. Nothing for an issue that isn't in progress or in review,
 * or a PR still open.
 */
export function prOps(issue: Issue, state: PrState, issues: Issue[]): Op[] {
  const url = issue.links?.pr;
  if (!url || !isLive(issue)) return [];
  if (state === "merged") {
    const open = childrenOf(issues, issue.key).some((c) => c.status !== "done" && c.status !== "canceled");
    return open ? noteOnce(issue, childrenNote(url)) : [{ kind: "note", key: issue.key, text: `The linked pull request was merged: ${url}.` }, { kind: "update", key: issue.key, status: "done" }];
  }
  return state === "closed" ? noteOnce(issue, closedNote(url)) : [];
}
