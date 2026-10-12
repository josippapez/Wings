/** The PR tracker's shared state: every map the modules read and write, and small lookups over them. */
import type { Cli, Diff, FailureDetail, Link, Problem, Remote, Status, StatusEntry } from "./types";

export const state = { panes: [] as WingsPane[] };
/** cwd → { kind: "github" | "azure" | "gitlab", origin, ...provider fields } | { kind: "none" } */
export const remotes = new Map<string, Remote>();
/** paneId → PR ref { provider, url, number, repo, ...provider fields } */
export const links = new Map<string, Link>();
/** paneId → { ahead, behind }: commits to push and pull on the pane's branch, as of the last fetch. */
export const sync = new Map<string, { ahead: number; behind: number }>();
/** `${origin} ${branch}` → { at, link: Promise }, so panes on the same branch share one network lookup. */
export const lookups = new Map<string, { at: number; link: Promise<Link | null> }>();
/** PR url → normalized status, or { error } */
export const status = new Map<string, StatusEntry>();
/** PR url → its running status refresh. */
export const inFlight = new Map<string, Promise<void>>();
/** PRs refreshed because you clicked Refresh; background polls don't show a spinner. */
export const manual = new Set<string>();
/** PR url → { key, patch, comments }, fetched ahead so "View changes" opens instantly. */
export const diffs = new Map<string, Diff>();
export const diffLoads = new Map<string, Promise<Diff>>();
/** Open diff viewer id → PR url, so a reply typed in it reaches the right PR. */
export const viewers = new Map<string, string>();
/** "github", an Azure org or `gitlab:<host>` → { login, missing, message }, when its CLI can't be used there. `missing` names a CLI that isn't installed. */
export const problems = new Map<string | undefined, Problem>();
/** Provider whose sign-in is running → { code, url } once the CLI prints them. */
export const signingIn = new Map<string, { code?: string }>();
export const PROVIDERS: Record<string, { name: string; tone: Tone }> = {
  github: { name: "GitHub", tone: "merged" },
  azure: { name: "Azure DevOps", tone: "warning" },
  gitlab: { name: "GitLab", tone: "info" },
};
export const CLIS: Record<Cli, string> = { gh: "GitHub CLI", az: "Azure CLI", glab: "GitLab CLI" };

/** PR url → { key, items, list }: details of the failed checks `key` names. `items` is the load, `list` its result. */
export const failures = new Map<string, { key: string; items: Promise<FailureDetail[]>; list: FailureDetail[] | null }>();

export const failKey = (st: Status) => st.failing.map((f) => f.id).join(" ");

export function linkFor(url: string) {
  return [...links.values()].find((l) => l.url === url);
}

/** GitLab numbers merge requests `!12`; the others use `#12`. */
export const refOf = (link: Link) => `${link.provider === "gitlab" ? "!" : "#"}${link.number}`;
