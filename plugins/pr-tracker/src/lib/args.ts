/** A PR to and from the args its tab is opened with. No `wings`, no I/O, so node tests drive it. */
import { PROVIDERS } from "./providers";
import type { Link, PrArgs, Provider, Remote } from "./types";

/** The args a "pr" tab opens with: the PR's own fields, and the remote of a provider that needs more than the URL. */
export function argsOf(link: Link, title: string, cwd: string): PrArgs {
  const base = { provider: link.provider, url: link.url, number: link.number, repo: link.repo, title, cwd };
  return "remote" in link ? { ...base, remote: link.remote } : base;
}

const isProvider = (value: unknown): value is Provider => typeof value === "string" && Object.hasOwn(PROVIDERS, value);

/** The args a tab was opened with, or null when they aren't a pull request's. They come from Wings' saved state as JSON, so they are checked, not trusted. */
export function readArgs(raw: unknown): PrArgs | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { provider, url, number, repo, title, cwd, remote } = raw as Record<string, unknown>;
  if (!isProvider(provider) || typeof url !== "string" || typeof number !== "number" || typeof repo !== "string" || typeof title !== "string" || typeof cwd !== "string") return null;
  const args: PrArgs = { provider, url, number, repo, title, cwd };
  if (remote !== undefined) {
    if (typeof remote !== "object" || remote === null || typeof (remote as { kind?: unknown }).kind !== "string") return null;
    args.remote = remote as Remote;
  }
  return args;
}
