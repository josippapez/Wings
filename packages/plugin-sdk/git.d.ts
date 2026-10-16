/// <reference path="./wings.d.ts" />
/** What `wings.exec` resolves: `code` is null when the program was killed by a signal. */
export interface GitResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

/** Runs `git args` in `cwd` and resolves what `wings.exec` does: the exit code, stdout and stderr, exactly as git gave them. */
export function gitRun(cwd: string, args: string[]): Promise<GitResult>;

/**
 * Runs `git args` in `cwd` and resolves its stdout with the whitespace around it trimmed. `raw` keeps it exactly as git
 * printed it, for a patch or a `-z` list. A non-zero exit throws git's first line of stderr, or `git <command> failed`.
 */
export function git(cwd: string, args: string[], options?: { raw?: boolean }): Promise<string>;

/**
 * Runs `git args` in `cwd` and resolves its trimmed stdout, or null when git exits non-zero or `wings.exec` rejects
 * (git refused the call, or the plugin isn't approved yet). It never rejects, so nothing caches a rejection.
 */
export function gitOk(cwd: string, args: string[]): Promise<string | null>;

/**
 * The top folder of the repo that `cwd` is in, or null outside one. A found root is kept for good. A folder git says
 * is in no repo is believed for a minute, so a poll doesn't ask again for it. A call git refused (the plugin isn't
 * approved yet) is kept for neither.
 */
export const repoRoot: {
  (cwd: string): Promise<string | null>;
  /** Forgets what was found for `cwd`, or for every folder when left out, so the next call asks git again. For a Refresh button. */
  forget(cwd?: string): void;
};

/** The branch a repo's "this branch" is compared against: `origin/HEAD` as the remote set it, else `origin/main`, `origin/master`, `main` or `master`. Null when none exists. Kept once found, until `forget`. */
export const defaultBranch: {
  (root: string): Promise<string | null>;
  /** Forgets what was found for `root`, or for every repo when left out, so the next call asks git again. */
  forget(root?: string): void;
};

/**
 * Calls `handler(projectIds)` when the repo of a project changes: a commit, checkout, pull, fetch, push, amend or
 * reset, from anywhere. Needs a `git` entry in `permissions.exec`. Events are held while the page is out of view or
 * Wings isn't focused, and handled once, with every id that came meanwhile, when it is back. A burst while `handler`
 * runs makes it run once more. `handler(projectIds, roots)` also gets the `repoRoot` of each changed project. The
 * `defaultBranch` of each changed repo is forgotten first, so `handler` reads it fresh. Edits to files in the working tree don't send an event, so a plugin that shows them keeps a slow poll.
 * `wings` is for tests. Returns a function that stops it. Needs Wings 0.1.50.
 */
export function onGitChange(
  handler: (projectIds: string[], roots: string[]) => void | Promise<void>,
  options?: { wings?: Pick<Wings, "onGitChange" | "onFocus" | "focused" | "projects"> },
): () => void;

/**
 * The id of the project whose repo `cwd` is in, or null when no project has that repo, which means Wings isn't
 * watching it and the caller keeps polling. `projects` is `wings.projects()`. Wings watches a project's repo
 * from its folder up, and the answer is the project whose `repoRoot` is the same as `cwd`'s.
 */
export function watchedProject(cwd: string, projects: { id: string; path: string }[]): Promise<string | null>;

/** What to call a checkout: its branch, else "detached at" and its short commit, else "no commits". `head` is `git rev-parse --verify --quiet HEAD`, null in a repo with no commits. */
export function branchLabel(branch: string | null | undefined, head: string | null | undefined): string;

/** The forge a remote URL points at, and where in it. */
export type Remote =
  /** `org` is the organization's URL, like `https://dev.azure.com/acme`, which `az` takes as `--organization`. */
  | { kind: "azure"; org: string; project: string; repo: string }
  /** `path` is `owner/repo`. `host` is `github.com`: another host is `unknown`. */
  | { kind: "github"; host: string; path: string }
  /** `path` is the project's path, with its groups: `group/subgroup/project`. */
  | { kind: "gitlab"; host: string; path: string }
  | { kind: "bitbucket"; workspace: string; repo: string }
  /** Codeberg. */
  | { kind: "gitea"; host: string; owner: string; repo: string }
  /** A host whose name doesn't say which forge it is, like `git.corp.example`. A CLI that is signed in to it can tell. */
  | { kind: "unknown"; host: string; path: string }
  /** Not a remote URL, or a Bitbucket or Codeberg one that doesn't name a repository. `host` is set for the latter. */
  | { kind: "none"; host?: string };

/**
 * Which forge a remote URL points at, from its form: Azure DevOps (ssh.dev.azure.com, dev.azure.com, `{org}.visualstudio.com`
 * and vs-ssh.visualstudio.com, the older ssh host that still works), github.com, gitlab.com and hosts with "gitlab" in
 * their name, Bitbucket Cloud and Codeberg. Any other host is `unknown`, and what isn't a remote URL is `none`.
 */
export function parseRemote(url: string): Remote;

/**
 * An Azure DevOps organization at its dev.azure.com address, for a call that must name that host, like a `post` entry
 * in the manifest: an `az` entry can't hold a `*` in its host, so it can't name every `{org}.visualstudio.com`. An
 * organization's REST APIs accept both forms (learn.microsoft.com/en-us/azure/devops/extend/develop/work-with-urls).
 * Takes the `org` of a parsed Azure remote.
 */
export function azureApiOrg(org: string): string;
