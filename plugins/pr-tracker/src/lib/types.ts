/** The shapes the PR tracker keeps in its maps, and the parts of the CLIs' JSON it reads. */

export type Provider = "github" | "azure" | "gitlab" | "bitbucket" | "gitea";
export type Cli = "gh" | "az" | "glab";
export type Icon = NonNullable<WingsBadge["icon"]>;

export interface AzureRemote {
  kind: "azure";
  org: string;
  project: string;
  repo: string;
  origin?: string;
}

export interface GitlabRemote {
  kind: "gitlab";
  host: string;
  path: string;
  origin?: string;
}

/** `host` is github.com or a GitHub Enterprise Server host that `gh` is signed in to; `path` is `owner/repo`. */
export interface GithubRemote {
  kind: "github";
  host: string;
  path: string;
  origin?: string;
}

export interface BitbucketRemote {
  kind: "bitbucket";
  workspace: string;
  repo: string;
  origin?: string;
}

/** Gitea and Forgejo, like codeberg.org. */
export interface GiteaRemote {
  kind: "gitea";
  host: string;
  owner: string;
  repo: string;
  origin?: string;
}

/** A host that no provider's name gives away: `gh` or `glab` being signed in to it says what it is. */
export interface UnknownRemote {
  kind: "unknown";
  host: string;
  path: string;
  origin?: string;
}

export type Remote = GithubRemote | AzureRemote | GitlabRemote | BitbucketRemote | GiteaRemote | UnknownRemote | { kind: "none"; origin?: string; org?: undefined; host?: string };

export interface GithubLink {
  provider: "github";
  url: string;
  number: number;
  repo: string;
}

export interface AzureLink {
  provider: "azure";
  url: string;
  number: number;
  repo: string;
  remote: AzureRemote;
  cwd: string;
  raw: AzurePr;
}

export interface GitlabLink {
  provider: "gitlab";
  url: string;
  number: number;
  repo: string;
  remote: GitlabRemote;
}

export interface BitbucketLink {
  provider: "bitbucket";
  url: string;
  number: number;
  repo: string;
  remote: BitbucketRemote;
  /** The project's folder, which the diff is made in. */
  cwd: string;
}

export interface GiteaLink {
  provider: "gitea";
  url: string;
  number: number;
  repo: string;
  remote: GiteaRemote;
}

/** A remote whose provider is settled. */
export type KnownRemote = Exclude<Remote, UnknownRemote>;

/** PR ref { provider, url, number, repo, ...provider fields } */
export type Link = GithubLink | AzureLink | GitlabLink | BitbucketLink | GiteaLink;

export type Result = "passed" | "failed" | "running";
export type Counts = Record<Result, number>;

/** A failed check as the status lists it. Which fields are set depends on the provider. */
export interface Failing {
  id: string | number;
  name: string | undefined;
  url?: string | null;
  job?: string | number;
  summary?: string | null;
  build?: number | null;
  project?: number;
}

/** What a check is doing. `skipped` is not run, not applicable or waiting for someone to start it. */
export type CheckState = "failed" | "running" | "queued" | "passed" | "skipped";

/** One check or policy of a PR, in the shape the Checks view shows whatever the provider. */
export interface CheckItem {
  id: string;
  name: string;
  state: CheckState;
  /** True when it blocks the merge, false when it doesn't, null where the provider doesn't say. */
  required: boolean | null;
  /** What sort of check the provider says it is: a workflow, a build, a status check, required reviewers... */
  kind: string | null;
  /** When it finished, else when it started (ISO). */
  at: string | null;
  seconds: number | null;
  /** Where the provider shows it. Only https is ever opened. */
  url: string | null;
  /** Why it is in this state, from the provider's data: "2 of 3 required reviewers approved". */
  note: string | null;
  /** Set where its log can be fetched, which `failureOf` takes. */
  failing?: Failing;
  /** The page fills the note from what it has loaded: a comment requirement counts the open threads. */
  needs?: "unresolved";
}

/** An open PR from a repo's list, as a project's Pull requests section shows it. */
export interface ListedPr {
  link: Link;
  title: string;
  /** The source branch, which an agent pane's branch is matched against. */
  head: string;
  /** You wrote it. */
  mine: boolean;
  /** Your review is asked for. */
  reviewing: boolean;
  /** Who wrote it, where the list says. */
  author: string | null;
  /** From the list alone, so GitLab and Azure DevOps have no check counts until the PR's own status loads. */
  status: Status;
  /** Comments and reviews so far, or null where the list doesn't say. */
  activity: number | null;
}

/**
 * What a "pr" tab is opened with (`wings.openView("pr", { as: "tab", args })`, read with `wings.viewArgs()`): enough
 * to find the PR again without the pane or the list that showed it. `cwd` is the project, where Azure DevOps diffs from.
 */
export interface PrArgs {
  provider: Provider;
  url: string;
  number: number;
  repo: string;
  title: string;
  cwd: string;
  remote?: AzureRemote | GitlabRemote | BitbucketRemote | GiteaRemote;
}

/** Normalized status of a PR. */
export interface Status {
  error?: undefined;
  key: string;
  title: string;
  label: string;
  tone: Tone;
  icon: Icon;
  checks: Counts;
  review: string;
  failing: Failing[];
  /** Every check, from the data the status already loaded. */
  items?: CheckItem[];
  changes: string | null;
  branch: string;
  updated: string;
  merge?: string | null;
  base?: string;
  head?: string;
  baseCommit?: string | null;
  headCommit?: string | null;
}

/** A status that failed to load, with none of a loaded status's fields. */
export type StatusError = { error: string } & { [K in Exclude<keyof Status, "error">]?: undefined };

export type StatusEntry = Status | StatusError;

/** A failed check's details: its failed step, the line that says why, and the end of its log. */
export interface FailureDetail {
  check: string | undefined;
  step?: string | null;
  summary: string;
  log: string;
  url: string | null | undefined;
}

export interface Comment extends WingsComment {
  /** The GitLab discussion a note belongs to, which a reply goes to. */
  discussion?: string;
  /** On a thread's first comment, where the provider keeps thread states: open ("active") or done ("resolved"), with its own word for it. */
  thread?: { state: "active" | "resolved"; label: string };
  /** On a code thread's first comment, the commented lines and the PR iteration they were written on, where the provider says. */
  range?: { start: number; end: number; iteration?: number };
  /** Written by the provider itself (a policy update, a status change) rather than a person. Only the PR tab asks for these. */
  system?: boolean;
}

/** A reviewer's latest verdict on a PR. */
export interface Reviewer {
  name: string;
  vote: "approved" | "changes" | "waiting" | "none";
}

export interface Diff {
  key: string | undefined;
  patch: string;
  comments: Comment[];
}

export interface Problem {
  login?: boolean;
  /** Signing in means pasting a token in Settings, since there is no CLI to sign in with. */
  token?: boolean;
  missing?: Cli;
  message: string;
}

// ---------- CLI output ----------

export interface AzurePr {
  pullRequestId: number;
  status: string;
  title: string;
  isDraft?: boolean;
  mergeStatus?: string;
  reviewers?: { vote: number; displayName?: string; uniqueName?: string; isRequired?: boolean }[];
  repository?: { webUrl?: string };
  lastMergeSourceCommit?: { commitId: string };
  lastMergeTargetCommit?: { commitId: string };
  description?: string;
  createdBy?: { displayName?: string; uniqueName?: string };
  creationDate: string;
  closedDate?: string;
  sourceRefName?: string;
  targetRefName?: string;
}

export interface AzurePolicy {
  status: string;
  startedDate?: string;
  completedDate?: string;
  configuration?: {
    isEnabled?: boolean;
    isBlocking?: boolean;
    type?: { id?: string; displayName?: string };
    settings?: Record<string, unknown> & { displayName?: string };
  };
  context?: { buildId?: number; buildDefinitionName?: string };
}
