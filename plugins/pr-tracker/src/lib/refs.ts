/** A PR's ref to and from the args of its tab, and what the tab shows of it. */
import { azureComments, azureLink, azureOverview, azureReply, azureShow } from "./azure";
import { bitbucketComments, bitbucketOverview } from "./bitbucket";
import { giteaComments, giteaOverview } from "./gitea";
import { githubComments, githubOverview, githubReply } from "./github";
import { gitlabComments, gitlabOverview, gitlabReply } from "./gitlab";
import { PROVIDERS } from "./state";
import type { AzureLink, AzureRemote, BitbucketLink, BitbucketRemote, Comment, GiteaLink, GiteaRemote, GitlabLink, GitlabRemote, Link, PrArgs, Status } from "./types";

export function argsOf(link: Link, title: string, cwd: string): PrArgs {
  const base = { provider: link.provider, url: link.url, number: link.number, repo: link.repo, title, cwd };
  return link.provider === "github" ? base : { ...base, remote: { ...link.remote, origin: undefined } };
}

/** The link a tab's args name. Azure DevOps looks the PR up again, because its status needs the PR's own fields. */
export async function linkOf(args: PrArgs): Promise<Link> {
  if (args.provider === "github") return { provider: "github", url: args.url, number: args.number, repo: args.repo };
  if (args.provider === "gitlab") return { provider: "gitlab", url: args.url, number: args.number, repo: args.repo, remote: args.remote as GitlabRemote };
  if (args.provider === "bitbucket") return { provider: "bitbucket", url: args.url, number: args.number, repo: args.repo, remote: args.remote as BitbucketRemote, cwd: args.cwd };
  if (args.provider === "gitea") return { provider: "gitea", url: args.url, number: args.number, repo: args.repo, remote: args.remote as GiteaRemote };
  const remote = args.remote as AzureRemote;
  return { ...azureLink(await azureShow(remote, args.number), remote, args.cwd), url: args.url };
}

export interface Overview {
  status: Status;
  author: string;
  body: string;
}

/** The status, author and description of the PR, from whichever provider it's on. */
export function overviewOf(link: Link): Promise<Overview> {
  return { github: githubOverview, azure: (l: Link) => azureOverview(l as AzureLink), gitlab: (l: Link) => gitlabOverview(l as GitlabLink), bitbucket: (l: Link) => bitbucketOverview(l as BitbucketLink), gitea: (l: Link) => giteaOverview(l as GiteaLink) }[link.provider](link as never);
}

export function commentsOf(link: Link): Promise<Comment[]> {
  return { github: githubComments, azure: azureComments, gitlab: gitlabComments, bitbucket: bitbucketComments, gitea: giteaComments }[link.provider](link as never);
}

/** Posts a reply to a thread. Bitbucket and Codeberg threads are read-only here: their comments carry no `canReply`, so this is a guard. */
export function replyWith(link: Link, root: Comment, body: string): Promise<Comment> {
  switch (link.provider) {
    case "github":
      return githubReply(link, root, body);
    case "azure":
      return azureReply(link, root, body);
    case "gitlab":
      return gitlabReply(link, root, body);
    default:
      return Promise.reject(new Error(`Replies to ${PROVIDERS[link.provider].name} comments aren't supported yet. Open the pull request there to reply.`));
  }
}
