/** A PR's ref to and from the args of its tab, and what the tab shows of it. */
import { azureComments, azureLink, azureOverview, azureShow } from "./azure";
import { githubComments, githubOverview } from "./github";
import { gitlabComments, gitlabOverview } from "./gitlab";
import type { AzureLink, AzureRemote, Comment, GitlabLink, GitlabRemote, Link, PrArgs, Status } from "./types";

export function argsOf(link: Link, title: string, cwd: string): PrArgs {
  const base = { provider: link.provider, url: link.url, number: link.number, repo: link.repo, title, cwd };
  return link.provider === "github" ? base : { ...base, remote: { ...link.remote, origin: undefined } };
}

/** The link a tab's args name. Azure DevOps looks the PR up again, because its status needs the PR's own fields. */
export async function linkOf(args: PrArgs): Promise<Link> {
  if (args.provider === "github") return { provider: "github", url: args.url, number: args.number, repo: args.repo };
  if (args.provider === "gitlab") return { provider: "gitlab", url: args.url, number: args.number, repo: args.repo, remote: args.remote as GitlabRemote };
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
  return { github: githubOverview, azure: (l: Link) => azureOverview(l as AzureLink), gitlab: (l: Link) => gitlabOverview(l as GitlabLink) }[link.provider](link as never);
}

export function commentsOf(link: Link): Promise<Comment[]> {
  return { github: githubComments, azure: azureComments, gitlab: gitlabComments }[link.provider](link as never);
}
