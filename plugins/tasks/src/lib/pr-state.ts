// A linked pull request's state, read with the host's CLI: how to ask for it, what the answer means, and what that does to
// an issue that is still in progress or in review.

import type { Remote } from "wings-plugin-sdk/git";
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

/** A PR found for a branch: its link and state. */
export interface FoundPr {
  url: string;
  state: PrState;
}

/** The program and arguments that list the PRs whose source branch is `branch`, open or not, run in the project's folder. Null for a host other than github.com and Azure DevOps, or a branch that could pass for a flag. */
export function branchPrCommand(remote: Remote, branch: string): { program: "gh" | "az"; args: string[] } | null {
  if (!branch || branch.startsWith("-")) return null;
  if (remote.kind === "github" && remote.host === "github.com") return { program: "gh", args: ["pr", "list", "--head", branch, "--state", "all", "--limit", "5", "--json", "url,state"] };
  if (remote.kind === "azure") return { program: "az", args: ["repos", "pr", "list", "--organization", remote.org, "--project", remote.project, "--repository", remote.repo, "--source-branch", branch, "--status", "all", "--top", "5", "--output", "json"] };
  return null;
}

/** The PR of a branch from what that command printed, newest first: an open one, else the newest. Null when there is none. */
export function branchPrOf(remote: Remote, output: string): FoundPr | null {
  let list: unknown;
  try {
    list = JSON.parse(output);
  } catch {
    return null;
  }
  if (!Array.isArray(list)) return null;
  const found = list.flatMap((pr): FoundPr[] => {
    if (typeof pr !== "object" || pr === null) return [];
    if (remote.kind === "github") {
      const { url, state } = pr as { url?: unknown; state?: unknown };
      const word = prStateOf(typeof state === "string" ? state : "");
      return typeof url === "string" && word ? [{ url, state: word }] : [];
    }
    if (remote.kind === "azure") {
      const { pullRequestId, status, repository } = pr as { pullRequestId?: unknown; status?: unknown; repository?: { webUrl?: unknown } };
      const word = prStateOf(typeof status === "string" ? status : "");
      const base = typeof repository?.webUrl === "string" ? repository.webUrl : `${remote.org}/${encodeURIComponent(remote.project)}/_git/${encodeURIComponent(remote.repo)}`;
      return typeof pullRequestId === "number" && word ? [{ url: `${base}/pullrequest/${pullRequestId}`, state: word }] : [];
    }
    return [];
  });
  return found.find((f) => f.state === "open") ?? found[0] ?? null;
}

/** What the PR tracker's `pr-state` event says: a tracked PR merged or closed. */
export interface PrStateEvent {
  url: string;
  state: "merged" | "closed";
  sourceBranch: string;
  projectId: string;
}

/** The event's fields, or null when what arrived isn't one. It comes from another plugin as JSON, so it is checked. */
export function prStateEventOf(data: unknown): PrStateEvent | null {
  const { url, state, sourceBranch, projectId } = (data ?? {}) as Record<string, unknown>;
  if (typeof url !== "string" || (state !== "merged" && state !== "closed") || typeof sourceBranch !== "string" || typeof projectId !== "string" || !url || !sourceBranch) return null;
  return { url, state, sourceBranch, projectId };
}

/** Whether a PR that merged or closed is this live issue's: its link is that PR, or it has no link and its branch is the PR's source branch. */
export function isPrOf(issue: Issue, pr: { url: string; sourceBranch: string }): boolean {
  if (!isLive(issue)) return false;
  const linked = issue.links?.pr;
  return linked ? linked === pr.url : issue.links?.branch === pr.sourceBranch;
}

/** What finding a PR for a live issue does: link it when the issue has no link yet, then what its state does (`prOps`). */
export function foundOps(issue: Issue, found: FoundPr, issues: Issue[]): Op[] {
  const link: Op[] = issue.links?.pr === found.url ? [] : [{ kind: "update", key: issue.key, pr: found.url }];
  return [...link, ...prOps({ ...issue, links: { ...issue.links, pr: found.url } }, found.state, issues)];
}
