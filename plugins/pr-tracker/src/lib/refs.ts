/** A PR's ref to and from the args of its tab, and what the tab shows of it. */
import { azureComments, azureLink, azureOverview, azureReply, azureShow, azureVote } from "./azure";
import { bitbucketApprove, bitbucketComments, bitbucketOverview, bitbucketReply } from "./bitbucket";
import { giteaApprove, giteaComments, giteaOverview, giteaReply } from "./gitea";
import { githubApprove, githubComments, githubOverview, githubReply } from "./github";
import { gitlabApprove, gitlabComments, gitlabOverview, gitlabReply } from "./gitlab";
import type { AzureVote } from "./vote";
import type { AzureLink, AzureRemote, BitbucketLink, BitbucketRemote, Comment, GiteaLink, GiteaRemote, GitlabLink, GitlabRemote, Link, PrArgs, Provider, Reviewer, Status } from "./types";

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
  /** When the PR was opened, where the provider says. */
  createdAt?: string;
  /** Each reviewer's latest verdict, where the provider says; the status's `review` text covers the rest. */
  reviewers?: Reviewer[];
}

/** The status, author and description of the PR, from whichever provider it's on. */
export function overviewOf(link: Link): Promise<Overview> {
  return { github: githubOverview, azure: (l: Link) => azureOverview(l as AzureLink), gitlab: (l: Link) => gitlabOverview(l as GitlabLink), bitbucket: (l: Link) => bitbucketOverview(l as BitbucketLink), gitea: (l: Link) => giteaOverview(l as GiteaLink) }[link.provider](link as never);
}

/** The PR's comments for the tab, with the providers' own notes (policy updates, system notes), which the diff viewer leaves out. */
export function commentsOf(link: Link): Promise<Comment[]> {
  return { github: githubComments, azure: (l: Link) => azureComments(l as AzureLink, true), gitlab: (l: Link) => gitlabComments(l as GitlabLink, true), bitbucket: bitbucketComments, gitea: giteaComments }[link.provider](link as never);
}

/** Posts a reply to a thread. Azure DevOps, GitLab and GitHub reply in the thread; Bitbucket under the comment; Codeberg as a new comment that quotes it. */
export function replyWith(link: Link, root: Comment, body: string): Promise<Comment> {
  switch (link.provider) {
    case "github":
      return githubReply(link, root, body);
    case "azure":
      return azureReply(link, root, body);
    case "gitlab":
      return gitlabReply(link, root, body);
    case "bitbucket":
      return bitbucketReply(link, root, body);
    case "gitea":
      return giteaReply(link, root, body);
  }
}

/** Providers that can approve with one scoped call. Azure DevOps votes instead, through `voteWith`. */
export const canApprove = (provider: Provider) => provider !== "azure";

/** Approves the pull request as the signed-in user. `sha` is the head commit the user was looking at (GitLab approves only that one). */
export async function approveWith(link: Link, sha: string): Promise<void> {
  switch (link.provider) {
    case "github":
      return githubApprove(link);
    case "gitlab":
      return gitlabApprove(link, sha);
    case "bitbucket":
      return bitbucketApprove(link);
    case "gitea":
      return giteaApprove(link);
    case "azure":
      throw new Error("Vote on an Azure DevOps pull request with the Vote menu.");
  }
}

/** Casts an Azure DevOps vote. */
export async function voteWith(link: Link, vote: AzureVote): Promise<void> {
  if (link.provider !== "azure") throw new Error("Only Azure DevOps pull requests take a vote.");
  return azureVote(link, vote);
}
