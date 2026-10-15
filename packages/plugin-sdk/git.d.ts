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

/** The top folder of the repo that `cwd` is in, or null outside one. Found roots are kept; a miss is asked again next time. */
export function repoRoot(cwd: string): Promise<string | null>;

/** The branch a repo's "this branch" is compared against: `origin/HEAD` as the remote set it, else `origin/main`, `origin/master`, `main` or `master`. Null when none exists. Kept once found. */
export function defaultBranch(root: string): Promise<string | null>;

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
 * Which forge a remote URL points at, from its form: Azure DevOps (ssh, dev.azure.com and visualstudio.com), github.com,
 * gitlab.com and hosts with "gitlab" in their name, Bitbucket Cloud and Codeberg. Any other host is `unknown`, and
 * what isn't a remote URL is `none`.
 */
export function parseRemote(url: string): Remote;
