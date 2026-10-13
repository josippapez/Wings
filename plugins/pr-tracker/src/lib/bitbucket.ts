/** Bitbucket Cloud: pull requests through the REST API 2.0 with an API token, the diff from local git, replies and approvals through the same token. */
import { git } from "./cli";
import { forgeJson, forgePost, forgeUser, type Forge } from "./forge";
import { bbComments, LIST_FIELDS, bbListed, bbOwn, bbStatusOf, encodeQuery, type BbComment, type BbPr, type BbStatus } from "./forges";
import { status } from "./state";
import type { BitbucketLink, BitbucketRemote, Comment, Link, ListedPr, Status } from "./types";

const API = "https://api.bitbucket.org/2.0/";
/** Bitbucket answers at most 50 pull requests a page. */
const PAGE = 50;
export const BITBUCKET: Forge = {
  key: "bitbucket",
  secret: "bitbucket",
  name: "Bitbucket",
  scopeHint: "The API token needs the read:pullrequest:bitbucket and read:user:bitbucket scopes.",
  writeHint: "The API token needs the write:pullrequest:bitbucket scope to reply and approve.",
};

interface Page<T> {
  values?: T[];
}

const repoPath = (r: BitbucketRemote) => `${API}repositories/${r.workspace}/${r.repo}`;
const getPage = async <T>(url: string) => (await forgeJson<Page<T>>(BITBUCKET, url)).values ?? [];

/** The account the token belongs to, by uuid, which Bitbucket filters and marks authors and reviewers with. */
const me = () => forgeUser<{ uuid?: string }>(BITBUCKET, `${API}user`, (u) => u.uuid);

const linkOf = (pr: BbPr, remote: BitbucketRemote, cwd: string): BitbucketLink => ({
  provider: "bitbucket",
  url: pr.links.html.href,
  number: pr.id,
  repo: `${remote.workspace}/${remote.repo}`,
  remote,
  cwd,
});

// ---------- finding the PR for a pane ----------

export async function findBitbucket(cwd: string, remote: BitbucketRemote, branch: string): Promise<Link | null> {
  // A quote or backslash in the branch name would end the filter's string early.
  if (/["\\]/.test(branch)) return null;
  const states = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"].map((s) => `state=${s}`).join("&");
  const q = encodeQuery(`source.branch.name="${branch}"`);
  const found = await getPage<BbPr>(`${repoPath(remote)}/pullrequests?q=${q}&${states}&sort=-updated_on&pagelen=20`);
  const prs = bbOwn(found, `${remote.workspace}/${remote.repo}`);
  // The open PR wins; otherwise the newest one for this branch.
  const pr = prs.find((p) => p.state === "OPEN") ?? prs[0];
  return pr ? linkOf(pr, remote, cwd) : null;
}

// ---------- status ----------

const prUrl = (link: BitbucketLink) => `${repoPath(link.remote)}/pullrequests/${link.number}`;

const load = (link: BitbucketLink) =>
  Promise.all([forgeJson<BbPr>(BITBUCKET, prUrl(link)), getPage<BbStatus>(`${prUrl(link)}/statuses?pagelen=100`).catch(() => [])]);

export async function bitbucketStatus(link: BitbucketLink): Promise<Status> {
  const [pr, statuses] = await load(link);
  return bbStatusOf(pr, statuses);
}

/** The status page: the status plus who wrote the PR and its description. */
export async function bitbucketOverview(link: BitbucketLink) {
  const [pr, statuses] = await load(link);
  return { status: bbStatusOf(pr, statuses), author: pr.author?.display_name ?? "unknown", body: pr.description ?? "" };
}

// ---------- the repo's open PRs ----------

/** Every open PR of the repo: one call, and a second for a repo with more than a page. The list has no check results, which the PR's own status adds. */
export async function bitbucketList(cwd: string, remote: BitbucketRemote): Promise<ListedPr[]> {
  const url = `${repoPath(remote)}/pullrequests?state=OPEN&sort=-updated_on&pagelen=${PAGE}&${LIST_FIELDS}`;
  const [user, first] = await Promise.all([me(), getPage<BbPr>(url)]);
  const prs = first.length === PAGE ? [...first, ...(await getPage<BbPr>(`${url}&page=2`))] : first;
  return bbListed(prs, user, (pr) => linkOf(pr, remote, cwd));
}

// ---------- diff and comments ----------

/**
 * The diff comes from local git, as for Azure DevOps: the API's own diff answers with a redirect that `wings.fetch`
 * doesn't follow. Only a PR from a branch of this repository is fetched; one from a fork isn't on `origin`.
 */
export async function bitbucketDiff(link: BitbucketLink) {
  const { base, head } = status.get(link.url) as Status;
  await git(link.cwd, ["fetch", "--quiet", "--end-of-options", "origin", base!, head!]);
  const patch = await git(link.cwd, ["diff", "--no-color", "--no-ext-diff", `origin/${base}...origin/${head}`]);
  return { patch: `${patch}\n`, comments: await bitbucketComments(link) };
}

export async function bitbucketComments(link: BitbucketLink): Promise<Comment[]> {
  return bbComments(await getPage<BbComment>(`${prUrl(link)}/comments?pagelen=100`), link.url);
}

// ---------- replies and approval ----------

/** Replies in the thread. The API threads a comment under `parent`, and a reply to a reply is moved under the thread's first comment on read. */
export async function bitbucketReply(link: BitbucketLink, root: Comment, body: string): Promise<Comment> {
  const created = JSON.parse(await forgePost(BITBUCKET, `${prUrl(link)}/comments`, { content: { raw: body }, parent: { id: root.id } })) as BbComment;
  return { ...bbComments([created], link.url)[0], replyTo: root.id, canReply: false };
}

/** Approves as the token's account. */
export async function bitbucketApprove(link: BitbucketLink): Promise<void> {
  await forgePost(BITBUCKET, `${prUrl(link)}/approve`);
}
