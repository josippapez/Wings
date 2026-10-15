/** A PR to the args its tab is opened with. No `wings`, no I/O, so node tests drive it. */
import type { Link, PrArgs } from "./types";

/** The args a "pr" tab opens with: the PR's own fields, and the remote of a provider that needs more than the URL. */
export function argsOf(link: Link, title: string, cwd: string): PrArgs {
  const base = { provider: link.provider, url: link.url, number: link.number, repo: link.repo, title, cwd };
  return "remote" in link ? { ...base, remote: link.remote } : base;
}
