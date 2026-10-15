/** Gitea and Forgejo (Codeberg): pull requests through `/api/v1` with an access token, the diff from the API's `.diff`, replies and approvals. */
import { real } from "../checklist";
import { counts, encodeQuery, labelOf, openOrNewest, plainFailure, withSecondPage } from "./shared";
import { forgeGet, forgeJson, forgePost, forgeUser, forgetUsers, type Account } from "./token";
import type { Forge } from "./types";
import type { CheckItem, CheckState, Comment, Failing, GiteaLink, GiteaRemote, ListedPr, Result, Status } from "../types";

/** Gitea answers at most 50 items a page (`MAX_RESPONSE_ITEMS`). */
const PAGE = 50;

/** One token per host, saved under a name made of the host's letters: `codeberg` for codeberg.org. */
const accountOf = (host: string): Account => ({
  key: `gitea:${host}`,
  secret: host.split(".")[0],
  name: host,
  scopeHint: "The access token needs the read:repository, read:issue and read:user scopes.",
  writeHint: "The access token needs the write:issue scope to reply and write:repository to approve.",
});

// ---------- the API's shapes and how they map ----------

interface GtUser {
  login?: string;
  full_name?: string;
}

/** The fields of a pull request the tracker reads, from `GET /api/v1/repos/{owner}/{repo}/pulls[/{index}]`. */
interface GtPr {
  number: number;
  title: string;
  body?: string;
  state: "open" | "closed";
  draft?: boolean;
  merged?: boolean;
  mergeable?: boolean;
  comments?: number;
  review_comments?: number;
  additions?: number;
  deletions?: number;
  changed_files?: number;
  user?: GtUser;
  requested_reviewers?: GtUser[] | null;
  head: { ref: string; sha: string; repo?: { full_name?: string } | null };
  base: { ref: string };
  updated_at: string;
  html_url: string;
}

/** A commit status from `.../commits/{sha}/status`: `success`, `failure`, `error`, `pending`, `warning` or `skipped`. */
interface GtCommitStatus {
  status: string;
  context?: string;
  description?: string;
  target_url?: string;
  created_at?: string;
  updated_at?: string;
}

interface GtReview {
  id: number;
  user?: GtUser;
  state: string;
  body?: string;
  dismissed?: boolean;
  submitted_at: string;
  html_url?: string;
}

interface GtComment {
  id: number;
  user?: GtUser;
  body?: string;
  created_at: string;
  html_url?: string;
}

const NO_CHECKS = { passed: 0, failed: 0, running: 0 };
const gtName = (u?: GtUser) => u?.full_name || u?.login || "unknown";

/** A commit status as a check of the Checks view. */
export function gtItem(s: GtCommitStatus): CheckItem {
  const state: CheckState = s.status === "success" || s.status === "warning" ? "passed" : s.status === "skipped" ? "skipped" : s.status === "pending" ? "running" : "failed";
  return {
    id: s.context ?? s.target_url ?? "check",
    name: s.context ?? "Check",
    state,
    required: null,
    kind: "Status check",
    at: real(s.updated_at) ?? real(s.created_at),
    seconds: null,
    url: s.target_url ?? null,
    note: s.description || (s.status === "warning" ? "Warning" : null),
  };
}

export function gtStatusOf(pr: GtPr, statuses: GtCommitStatus[], reviews: GtReview[]): Status {
  // `warning` doesn't block a merge, and `skipped` didn't run.
  const results = statuses.map((s): Result => (["success", "warning", "skipped"].includes(s.status) ? "passed" : s.status === "pending" ? "running" : "failed"));
  const failing = statuses
    .filter((_, i) => results[i] === "failed")
    .map((s): Failing => ({ id: s.context ?? s.target_url ?? "check", name: s.context, url: s.target_url ?? null, summary: s.description || null }));
  const checks = counts(results);
  // A reviewer's latest verdict counts, and a dismissed one doesn't.
  const latest = new Map<string, string>();
  for (const r of [...reviews].sort((a, b) => a.submitted_at.localeCompare(b.submitted_at))) {
    if (r.dismissed || !["APPROVED", "REQUEST_CHANGES"].includes(r.state)) continue;
    latest.set(r.user?.login ?? String(r.id), r.state);
  }
  const verdicts = [...latest.values()];
  const approved = verdicts.filter((v) => v === "APPROVED").length;
  const changes = verdicts.includes("REQUEST_CHANGES");
  const [label, tone, icon] = labelOf({ merged: pr.merged, closed: pr.state === "closed", draft: pr.draft, conflicts: pr.mergeable === false, checks, changes, approved: approved > 0 });
  const files = pr.changed_files;
  return {
    key: pr.head.sha,
    title: pr.title,
    label, tone, icon, checks, failing,
    items: statuses.map(gtItem),
    review: changes ? "Changes requested" : approved ? `${approved} approved` : "No approvals yet",
    changes: files === undefined ? null : `+${pr.additions ?? 0} −${pr.deletions ?? 0} in ${files} files`,
    branch: `${pr.head.ref} → ${pr.base.ref}`,
    updated: pr.updated_at,
  };
}

/** The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name. */
export function gtOwn(prs: GtPr[], fullName: string): GtPr[] {
  return prs.filter((p) => p.head.repo?.full_name?.toLowerCase() === fullName.toLowerCase());
}

/** Who a listed PR is for: written by `me`, or with `me` among the reviewers asked. */
function gtWho(pr: GtPr, me: string) {
  const lower = me.toLowerCase();
  return { mine: pr.user?.login?.toLowerCase() === lower, reviewing: (pr.requested_reviewers ?? []).some((u) => u.login?.toLowerCase() === lower) };
}

export function gtListed(prs: GtPr[], me: string, link: (pr: GtPr) => GiteaLink): ListedPr[] {
  return prs.map((pr): ListedPr => {
    const { mine, reviewing } = gtWho(pr, me);
    const [label, tone, icon] = labelOf({ draft: pr.draft, conflicts: pr.mergeable === false, checks: NO_CHECKS });
    return { link: link(pr), title: pr.title, head: pr.head.ref, mine, reviewing, author: pr.user ? gtName(pr.user) : null, status: { ...gtStatusOf(pr, [], []), label, tone, icon }, loaded: false, activity: (pr.comments ?? 0) + (pr.review_comments ?? 0) };
  });
}

/**
 * The comments on the whole PR: its conversation and the reviews' summaries. Line comments live under each
 * review (`.../reviews/{id}/comments`) and aren't read, so they show on the forge's own page only. Reviews get
 * negative ids, which can't clash with the conversation's.
 */
export function gtComments(comments: GtComment[], reviews: GtReview[], url: string): Comment[] {
  const general = [
    ...comments.map((c) => ({ id: c.id, user: c.user, body: c.body, at: c.created_at, url: c.html_url })),
    ...reviews.filter((r) => r.body && r.state !== "PENDING").map((r) => ({ id: -r.id, user: r.user, body: r.body, at: r.submitted_at, url: r.html_url })),
  ];
  return general.map((c): Comment => ({ id: c.id, replyTo: null, path: null, line: null, side: "additions", author: gtName(c.user), body: c.body ?? "", createdAt: c.at, url: c.url ?? url, canReply: true }));
}

/**
 * A reply to a Codeberg comment. The API has no threaded replies (a review comment can't take a parent, and the conversation
 * is flat), so a reply is a new comment on the pull request that quotes the first lines of what it answers.
 */
export function gtQuoted(root: Pick<Comment, "author" | "body">, reply: string): string {
  const lines = root.body.trim().split("\n").slice(0, 6).map((l) => `> ${l}`.trimEnd());
  const more = root.body.trim().split("\n").length > 6 ? ["> …"] : [];
  return `> **${root.author}** wrote:\n${[...lines, ...more].join("\n")}\n\n${reply}`;
}

// ---------- calling it ----------

const api = (r: { host: string; owner: string; repo: string }) => `https://${r.host}/api/v1/repos/${r.owner}/${r.repo}`;
const accountOfLink = (link: GiteaLink) => accountOf(link.remote.host);
const prUrl = (link: GiteaLink) => `${api(link.remote)}/pulls/${link.number}`;

const toLink = (pr: GtPr, remote: GiteaRemote): GiteaLink => ({ provider: "gitea", url: pr.html_url, number: pr.number, repo: `${remote.owner}/${remote.repo}`, remote });

async function load(link: GiteaLink) {
  const account = accountOfLink(link);
  const pr = await forgeJson<GtPr>(account, prUrl(link));
  const [combined, reviews] = await Promise.all([
    forgeJson<{ statuses?: GtCommitStatus[] }>(account, `${api(link.remote)}/commits/${pr.head.sha}/status?limit=50`).catch(() => ({ statuses: [] })),
    forgeJson<GtReview[]>(account, `${prUrl(link)}/reviews`).catch(() => []),
  ]);
  // Forgejo's actions give the check's page as a path on the host.
  const statuses = (combined.statuses ?? []).map((c) => (c.target_url?.startsWith("/") ? { ...c, target_url: `https://${link.remote.host}${c.target_url}` } : c));
  return { pr, statuses, reviews };
}

async function comments(link: GiteaLink): Promise<Comment[]> {
  const account = accountOfLink(link);
  const [conversation, reviews] = await Promise.all([
    // The conversation is the issue's comments, which need their own scope.
    forgeJson<GtComment[]>(account, `${api(link.remote)}/issues/${link.number}/comments`).catch((e: Error) => {
      throw /\(403\)/.test(e.message) ? new Error("Comments need the read:issue scope on the token.") : e;
    }),
    forgeJson<GtReview[]>(account, `${prUrl(link)}/reviews`),
  ]);
  return gtComments(conversation, reviews, link.url);
}

export const gitea: Forge<GiteaLink, GiteaRemote> = {
  problemKey: (remote) => accountOf(remote.host).key,
  repoKey: (remote) => `gitea:${remote.host}/${remote.owner}/${remote.repo}`,

  async find(_cwd, remote, branch) {
    const found = await forgeJson<GtPr[]>(accountOf(remote.host), `${api(remote)}/pulls?state=all&sort=recentupdate&limit=20&head=${encodeQuery(branch)}`);
    const pr = openOrNewest(gtOwn(found, `${remote.owner}/${remote.repo}`), (p) => p.state === "open");
    return pr ? toLink(pr, remote) : null;
  },

  /** One call for the repo's open PRs, kept when you wrote them or were asked to review them. */
  async list(_cwd, remote) {
    const account = accountOf(remote.host);
    const url = `${api(remote)}/pulls?state=open&sort=recentupdate&limit=${PAGE}`;
    const [user, first] = await Promise.all([forgeUser<{ login?: string }>(account, `https://${remote.host}/api/v1/user`, (u) => u.login), forgeJson<GtPr[]>(account, url)]);
    return gtListed(await withSecondPage(first, PAGE, () => forgeJson<GtPr[]>(account, `${url}&page=2`)), user, (pr) => toLink(pr, remote));
  },

  async linkOf({ remote, url, number, repo }) {
    if (remote?.kind !== "gitea") throw new Error("This tab doesn't say which Codeberg repository its pull request is in.");
    return { provider: "gitea", url, number, repo, remote };
  },

  async status(link) {
    const { pr, statuses, reviews } = await load(link);
    return gtStatusOf(pr, statuses, reviews);
  },

  async overview(link) {
    const { pr, statuses, reviews } = await load(link);
    return { status: gtStatusOf(pr, statuses, reviews), author: pr.user?.full_name || pr.user?.login || "unknown", body: pr.body ?? "" };
  },

  async diff(link) {
    const [patch, conversation] = await Promise.all([forgeGet(accountOfLink(link), `${prUrl(link)}.diff`, "text/plain"), comments(link)]);
    return { patch, comments: conversation };
  },

  comments,
  failure: plainFailure,

  /** Quotes the comment and posts it as a new comment on the pull request: the API has no reply to a comment. */
  async reply(link, root, body) {
    const created = JSON.parse(await forgePost(accountOfLink(link), `${api(link.remote)}/issues/${link.number}/comments`, { body: gtQuoted(root, body) })) as GtComment;
    return { ...gtComments([created], [], link.url)[0], replyTo: root.id, canReply: false };
  },

  /** Approves as the token's account; Gitea refuses it for your own pull request. */
  async approve(link) {
    await forgePost(accountOfLink(link), `${prUrl(link)}/reviews`, { event: "APPROVED" });
  },

  forget: forgetUsers,
};
