/** The shapes the PR tracker keeps in its maps, and the parts of the CLIs' JSON it reads. */

export type Provider = "github" | "azure" | "gitlab";
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

export type Remote = { kind: "github"; origin?: string } | AzureRemote | GitlabRemote | { kind: "none"; origin?: string; org?: undefined };

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

/** PR ref { provider, url, number, repo, ...provider fields } */
export type Link = GithubLink | AzureLink | GitlabLink;

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
}

export interface Diff {
  key: string | undefined;
  patch: string;
  comments: Comment[];
}

export interface Problem {
  login?: boolean;
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
  reviewers?: { vote: number }[];
  repository?: { webUrl?: string };
  lastMergeSourceCommit?: { commitId: string };
  lastMergeTargetCommit?: { commitId: string };
  creationDate: string;
  closedDate?: string;
  sourceRefName?: string;
  targetRefName?: string;
}

export interface AzurePolicy {
  status: string;
  configuration?: {
    isEnabled?: boolean;
    type?: { id?: string; displayName?: string };
    settings?: Record<string, unknown> & { displayName?: string };
  };
  context?: { buildId?: number; buildDefinitionName?: string };
}
