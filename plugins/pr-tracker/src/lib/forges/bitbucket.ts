/** Bitbucket Cloud: pull requests through the REST API 2.0 with an API token, the diff from local git, replies and approvals through the same token. */
import { cached } from "wings-plugin-sdk/async";
import { git } from "wings-plugin-sdk/git";
import { real } from "../checklist";
import { counts, encodeQuery, labelOf, openOrNewest, plainFailure, withSecondPage } from "./shared";
import { forgeJson, forgePost, forgeUser, forgetUsers, type Account } from "./token";
import type { Forge } from "./types";
import type { BitbucketLink, BitbucketRemote, CheckItem, CheckState, Comment, Failing, ListedPr, Result, Status } from "../types";

const API = "https://api.bitbucket.org/2.0/";
/** Bitbucket answers at most 50 pull requests a page. */
const PAGE = 50;
const ACCOUNT: Account = {
  key: "bitbucket",
  secret: "bitbucket",
  name: "Bitbucket",
  scopeHint: "The API token needs the read:pullrequest:bitbucket and read:user:bitbucket scopes.",
  writeHint: "The API token needs the write:pullrequest:bitbucket scope to reply and approve.",
};

// ---------- the API's shapes and how they map ----------

/** The list leaves out each PR's reviewers and participants as too costly, so they are asked for: `+` adds to the default fields and is sent as `%2B` (Bitbucket REST intro, "Partial responses"). */
export const LIST_FIELDS = "fields=%2Bvalues.participants,%2Bvalues.reviewers";

interface BbUser {
  uuid?: string;
  display_name?: string;
  nickname?: string;
  account_id?: string;
}

/** The fields of a pull request the tracker reads, from `GET /2.0/repositories/{workspace}/{repo}/pullrequests[/{id}]`. */
interface BbPr {
  id: number;
  title: string;
  description?: string;
  state: "OPEN" | "MERGED" | "DECLINED" | "SUPERSEDED";
  draft?: boolean;
  comment_count?: number;
  author?: BbUser;
  source: { branch: { name: string }; commit?: { hash: string } | null; repository?: { full_name?: string } | null };
  destination: { branch: { name: string } };
  participants?: { role: string; approved?: boolean; state?: string | null; user: BbUser }[];
  reviewers?: BbUser[];
  updated_on: string;
  links: { html: { href: string } };
}

/** A build result from `.../pullrequests/{id}/statuses`. */
interface BbStatus {
  key: string;
  state: "FAILED" | "INPROGRESS" | "STOPPED" | "SUCCESSFUL";
  type?: string;
  name?: string;
  description?: string;
  url?: string;
  created_on?: string;
  updated_on?: string;
}

interface BbComment {
  id: number;
  created_on: string;
  content?: { raw?: string };
  user?: BbUser;
  deleted?: boolean;
  parent?: { id: number } | null;
  inline?: { path: string; from?: number | null; to?: number | null } | null;
  links?: { html?: { href?: string } };
}

const NO_CHECKS = { passed: 0, failed: 0, running: 0 };
const bbName = (u?: BbUser) => u?.display_name ?? u?.nickname ?? "unknown";

function bbResults(statuses: BbStatus[]) {
  const results = statuses.map((s): Result => (s.state === "SUCCESSFUL" ? "passed" : s.state === "FAILED" || s.state === "STOPPED" ? "failed" : "running"));
  const failing = statuses
    .filter((_, i) => results[i] === "failed")
    .map((s): Failing => ({ id: s.key, name: s.name ?? s.key, url: s.url ?? null, summary: s.description || (s.state === "STOPPED" ? "Stopped" : null) }));
  return { checks: counts(results), failing };
}

/** A build result as a check of the Checks view. */
export function bbItem(s: BbStatus): CheckItem {
  const state: CheckState = s.state === "SUCCESSFUL" ? "passed" : s.state === "FAILED" || s.state === "STOPPED" ? "failed" : "running";
  return {
    id: s.key,
    name: s.name ?? s.key,
    state,
    required: null,
    kind: s.type === "build" ? "Build" : "Status check",
    at: real(s.updated_on) ?? real(s.created_on),
    seconds: null,
    url: s.url ?? null,
    note: s.description || (s.state === "STOPPED" ? "Stopped" : null),
  };
}

/** Reviewers who approved, and whether any asked for changes. Bitbucket marks a reviewer `approved`, or `state: "changes_requested"`. */
function bbReviews(pr: BbPr) {
  const reviewers = (pr.participants ?? []).filter((p) => p.role === "REVIEWER");
  return { approved: reviewers.filter((p) => p.approved).length, changes: reviewers.some((p) => p.state === "changes_requested") };
}

export function bbStatusOf(pr: BbPr, statuses: BbStatus[]): Status {
  const { checks, failing } = bbResults(statuses);
  const { approved, changes } = bbReviews(pr);
  const [label, tone, icon] = labelOf({ merged: pr.state === "MERGED", closed: pr.state === "DECLINED" || pr.state === "SUPERSEDED", draft: pr.draft, checks, changes, approved: approved > 0 });
  return {
    // A new push changes the source commit, which is what invalidates the cached diff.
    key: pr.source.commit?.hash ?? pr.updated_on,
    title: pr.title,
    label, tone, icon, checks, failing,
    items: statuses.map(bbItem),
    review: changes ? "Changes requested" : approved ? `${approved} approved` : "No approvals yet",
    changes: null,
    branch: `${pr.source.branch.name} → ${pr.destination.branch.name}`,
    updated: pr.updated_on,
    base: pr.destination.branch.name,
    head: pr.source.branch.name,
  };
}

/** The PRs whose branch is in this repository: a branch name alone also matches a fork's branch of the same name. */
export function bbOwn(prs: BbPr[], fullName: string): BbPr[] {
  return prs.filter((p) => p.source.repository?.full_name?.toLowerCase() === fullName.toLowerCase());
}

/** Who a listed PR is for: written by `me`, or with `me` among its reviewers. */
export function bbWho(pr: BbPr, me: string) {
  const reviewing = (pr.participants ?? []).some((p) => p.role === "REVIEWER" && p.user.uuid === me) || (pr.reviewers ?? []).some((u) => u.uuid === me);
  return { mine: pr.author?.uuid === me, reviewing };
}

/** Every open PR of a repo, in the list's order, each marked yours or waiting for you. `link` builds the ref, since a Bitbucket link carries the folder. */
export function bbListed(prs: BbPr[], me: string, link: (pr: BbPr) => BitbucketLink): ListedPr[] {
  return prs.map((pr): ListedPr => {
    const { mine, reviewing } = bbWho(pr, me);
    const { changes } = bbReviews(pr);
    const [label, tone, icon] = labelOf({ draft: pr.draft, checks: NO_CHECKS, changes });
    const status = { ...bbStatusOf(pr, []), label, tone, icon };
    return { link: link(pr), title: pr.title, head: pr.source.branch.name, mine, reviewing, author: pr.author ? bbName(pr.author) : null, status, loaded: false, activity: pr.comment_count ?? null, updatedAt: pr.updated_on ?? null };
  });
}

/** Comments in Bitbucket's order. A reply's `parent` is the comment it answers, which may itself be a reply, so each is moved under its thread's first comment. */
export function bbComments(values: BbComment[], url: string): Comment[] {
  const live = values.filter((c) => !c.deleted);
  const parents = new Map(live.map((c) => [c.id, c.parent?.id ?? null]));
  const rootOf = (id: number): number => {
    const parent = parents.get(id);
    return parent === null || parent === undefined || !parents.has(parent) ? id : rootOf(parent);
  };
  return live.map((c): Comment => {
    const root = rootOf(c.id);
    const inline = c.inline;
    return {
      id: c.id,
      replyTo: root === c.id ? null : root,
      path: inline?.path ?? null,
      line: inline ? (inline.to ?? inline.from ?? null) : null,
      side: inline?.to ? "additions" : inline ? "deletions" : "additions",
      author: bbName(c.user),
      by: c.user?.uuid,
      body: c.content?.raw ?? "",
      createdAt: c.created_on,
      url: c.links?.html?.href ?? url,
      canReply: root === c.id,
    };
  });
}

// ---------- calling it ----------

interface Page<T> {
  values?: T[];
}

const repoPath = (r: BitbucketRemote) => `${API}repositories/${r.workspace}/${r.repo}`;
const prUrl = (link: BitbucketLink) => `${repoPath(link.remote)}/pullrequests/${link.number}`;
const getPage = async <T>(url: string) => (await forgeJson<Page<T>>(ACCOUNT, url)).values ?? [];

/** The account the token belongs to, by uuid, which Bitbucket filters and marks authors and reviewers with. */
const me = () => forgeUser<{ uuid?: string }>(ACCOUNT, `${API}user`, (u) => u.uuid);

/** The token's account in full, for a mention, which Bitbucket writes with the account id or the uuid. */
const profile = cached(() => forgeJson<BbUser>(ACCOUNT, `${API}user`));

const toLink = (pr: BbPr, remote: BitbucketRemote, cwd: string): BitbucketLink => ({
  provider: "bitbucket",
  url: pr.links.html.href,
  number: pr.id,
  repo: `${remote.workspace}/${remote.repo}`,
  remote,
  cwd,
});

const load = (link: BitbucketLink) => Promise.all([forgeJson<BbPr>(ACCOUNT, prUrl(link)), getPage<BbStatus>(`${prUrl(link)}/statuses?pagelen=100`).catch(() => [])]);

const comments = async (link: BitbucketLink): Promise<Comment[]> => bbComments(await getPage<BbComment>(`${prUrl(link)}/comments?pagelen=100`), link.url);

export const bitbucket: Forge<BitbucketLink, BitbucketRemote> = {
  problemKey: () => ACCOUNT.key,
  repoKey: (remote) => `bitbucket:${remote.workspace}/${remote.repo}`,

  async find(cwd, remote, branch) {
    // A quote or backslash in the branch name would end the filter's string early.
    if (/["\\]/.test(branch)) return null;
    const states = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"].map((s) => `state=${s}`).join("&");
    const q = encodeQuery(`source.branch.name="${branch}"`);
    const found = await getPage<BbPr>(`${repoPath(remote)}/pullrequests?q=${q}&${states}&sort=-updated_on&pagelen=20`);
    const pr = openOrNewest(bbOwn(found, `${remote.workspace}/${remote.repo}`), (p) => p.state === "OPEN");
    return pr ? toLink(pr, remote, cwd) : null;
  },

  /** The account the token belongs to. Its uuid is what a comment's `by` is, and a mention is the account id or the uuid. */
  async me() {
    const u = await profile.get("user");
    const uuid = u.uuid ?? "";
    return { ids: [uuid], handles: [uuid, u.account_id, u.nickname].filter((h): h is string => Boolean(h)) };
  },

  /** Every open PR of the repo: one call, and a second for a repo with more than a page. The list has no check results, which the PR's own status adds. */
  async list(cwd, remote) {
    const url = `${repoPath(remote)}/pullrequests?state=OPEN&sort=-updated_on&pagelen=${PAGE}&${LIST_FIELDS}`;
    const [user, first] = await Promise.all([me(), getPage<BbPr>(url)]);
    return bbListed(await withSecondPage(first, PAGE, () => getPage<BbPr>(`${url}&page=2`)), user, (pr) => toLink(pr, remote, cwd));
  },

  async linkOf({ remote, url, number, repo, cwd }) {
    if (remote?.kind !== "bitbucket") throw new Error("This tab doesn't say which Bitbucket repository its pull request is in.");
    return { provider: "bitbucket", url, number, repo, remote, cwd };
  },

  async status(link) {
    const [pr, statuses] = await load(link);
    return bbStatusOf(pr, statuses);
  },

  async overview(link) {
    const [pr, statuses] = await load(link);
    return { status: bbStatusOf(pr, statuses), author: pr.author?.display_name ?? "unknown", body: pr.description ?? "" };
  },

  /**
   * The diff comes from local git, as for Azure DevOps: the API's own diff answers with a redirect that `wings.fetch`
   * doesn't follow. Only a PR from a branch of this repository is fetched; one from a fork isn't on `origin`.
   */
  async diff(link, { base, head }) {
    await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", base!, head!]);
    const patch = await git(link.cwd, ["diff", "--no-color", "--no-ext-diff", `origin/${base}...origin/${head}`], { raw: true });
    return { patch, comments: await comments(link) };
  },

  comments,
  failure: plainFailure,

  /** Replies in the thread. The API threads a comment under `parent`, and a reply to a reply is moved under the thread's first comment on read. */
  async reply(link, root, body) {
    const created = JSON.parse(await forgePost(ACCOUNT, `${prUrl(link)}/comments`, { content: { raw: body }, parent: { id: root.id } })) as BbComment;
    return { ...bbComments([created], link.url)[0], replyTo: root.id, canReply: false };
  },

  /** Approves as the token's account. */
  async approve(link) {
    await forgePost(ACCOUNT, `${prUrl(link)}/approve`);
  },

  forget() {
    forgetUsers();
    profile.clear();
  },
};
