/** Gitea and Forgejo (Codeberg): pull requests through `/api/v1` with an access token, the diff from the API's `.diff`, comments read-only. */
import { forgeGet, forgeJson, forgeUser, type Forge } from "./forge";
import { encodeQuery, gtComments, gtListed, gtOwn, gtStatusOf, type GtComment, type GtCommitStatus, type GtPr, type GtReview } from "./forges";
import type { Comment, GiteaLink, GiteaRemote, Link, ListedPr, Status } from "./types";

/** One token per host, saved under a name made of the host's letters: `codeberg` for codeberg.org. */
export const giteaForge = (host: string): Forge => ({
  key: `gitea:${host}`,
  secret: host.split(".")[0],
  name: host,
  scopeHint: "The access token needs the read:repository, read:issue and read:user scopes.",
});

const api = (r: { host: string; owner: string; repo: string }) => `https://${r.host}/api/v1/repos/${r.owner}/${r.repo}`;
const forgeOf = (link: GiteaLink) => giteaForge(link.remote.host);
const prUrl = (link: GiteaLink) => `${api(link.remote)}/pulls/${link.number}`;

const linkOf = (pr: GtPr, remote: GiteaRemote): GiteaLink => ({ provider: "gitea", url: pr.html_url, number: pr.number, repo: `${remote.owner}/${remote.repo}`, remote });

// ---------- finding the PR for a pane ----------

export async function findGitea(_cwd: string, remote: GiteaRemote, branch: string): Promise<Link | null> {
  const found = await forgeJson<GtPr[]>(giteaForge(remote.host), `${api(remote)}/pulls?state=all&sort=recentupdate&limit=20&head=${encodeQuery(branch)}`);
  const prs = gtOwn(found, `${remote.owner}/${remote.repo}`);
  // The open PR wins; otherwise the newest one for this branch.
  const pr = prs.find((p) => p.state === "open") ?? prs[0];
  return pr ? linkOf(pr, remote) : null;
}

// ---------- status ----------

async function load(link: GiteaLink) {
  const forge = forgeOf(link);
  const pr = await forgeJson<GtPr>(forge, prUrl(link));
  const [combined, reviews] = await Promise.all([
    forgeJson<{ statuses?: GtCommitStatus[] }>(forge, `${api(link.remote)}/commits/${pr.head.sha}/status?limit=50`).catch(() => ({ statuses: [] })),
    forgeJson<GtReview[]>(forge, `${prUrl(link)}/reviews`).catch(() => []),
  ]);
  // Forgejo's actions give the check's page as a path on the host.
  const statuses = (combined.statuses ?? []).map((c) => (c.target_url?.startsWith("/") ? { ...c, target_url: `https://${link.remote.host}${c.target_url}` } : c));
  return { pr, statuses, reviews };
}

export async function giteaStatus(link: GiteaLink): Promise<Status> {
  const { pr, statuses, reviews } = await load(link);
  return gtStatusOf(pr, statuses, reviews);
}

/** The status page: the status plus who wrote the PR and its description. */
export async function giteaOverview(link: GiteaLink) {
  const { pr, statuses, reviews } = await load(link);
  return { status: gtStatusOf(pr, statuses, reviews), author: pr.user?.full_name || pr.user?.login || "unknown", body: pr.body ?? "" };
}

// ---------- the repo's open PRs ----------

/** One call for the repo's open PRs, kept when you wrote them or were asked to review them. */
export async function giteaList(remote: GiteaRemote): Promise<ListedPr[]> {
  const forge = giteaForge(remote.host);
  const [user, prs] = await Promise.all([
    forgeUser<{ login?: string }>(forge, `https://${remote.host}/api/v1/user`, (u) => u.login),
    forgeJson<GtPr[]>(forge, `${api(remote)}/pulls?state=open&sort=recentupdate&limit=50`),
  ]);
  return gtListed(prs, user, (pr) => linkOf(pr, remote));
}

// ---------- diff and comments ----------

export async function giteaDiff(link: GiteaLink) {
  const [patch, comments] = await Promise.all([forgeGet(forgeOf(link), `${prUrl(link)}.diff`, "text/plain"), giteaComments(link)]);
  return { patch, comments };
}

export async function giteaComments(link: GiteaLink): Promise<Comment[]> {
  const forge = forgeOf(link);
  const [comments, reviews] = await Promise.all([
    // The conversation is the issue's comments, which need their own scope.
    forgeJson<GtComment[]>(forge, `${api(link.remote)}/issues/${link.number}/comments`).catch((e: Error) => {
      throw /\(403\)/.test(e.message) ? new Error("Comments need the read:issue scope on the token.") : e;
    }),
    forgeJson<GtReview[]>(forge, `${prUrl(link)}/reviews`),
  ]);
  return gtComments(comments, reviews, link.url);
}
