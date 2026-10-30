/** Who you are on a forge: `ids` match `Comment.by`, `handles` are what a mention in a comment's text is written with (`@handle`). */
export interface Me {
  ids: string[];
  handles: string[];
}

/** What a forge is to the rest of the tracker: one object that knows how to find, read and answer a pull request. */
import type { Cli } from "wings-plugin-sdk/cli";
import type { Push } from "../pushes";
import type { SnippetLine } from "../snippet";
import type { AzureVote } from "../vote";
import type { Comment, FailureDetail, Failing, Link, ListedPr, Overview, PrArgs, KnownRemote, Run, RunRef, RunStep, Status } from "../types";

/**
 * Why a forge can't be used for this account, which the pane's badge and the Pull requests block say: there is no
 * token, or it was refused (`token`), or the provider's CLI can't list the pull requests (`unavailable`). A CLI that is
 * missing or signed out is the CLI helper's `CliError` instead.
 */
export class AccountProblem extends Error {
  kind: "token" | "unavailable";

  constructor(kind: "token" | "unavailable", message: string) {
    super(message);
    this.name = "AccountProblem";
    this.kind = kind;
  }
}

/** The CLI a forge works through, which the badge offers to install and to sign in. */
export interface Login<R extends KnownRemote = KnownRemote> {
  readonly cli: Cli;
  /** What the badge and the block say when the CLI is missing or signed out. */
  words(remote: R): { signedOut: string; missing: string };
  /** Runs the CLI's login. `onCode` hears the one-time code a login prints, for the badge to show. */
  signIn(remote: R, onCode: (code: string) => void): Promise<void>;
}

/**
 * One provider, behind the same calls: GitHub, GitLab, Azure DevOps, Bitbucket and Codeberg each implement this in a
 * module of their own, and `registry.ts` hands out the one a link or a remote belongs to. `L` is the provider's link
 * and `R` its remote. The methods are declared as methods, so a `Forge<GithubLink, GithubRemote>` is a `Forge`, and a
 * link is only ever given to the forge that made it.
 */
export interface Forge<L extends Link = Link, R extends KnownRemote = KnownRemote> {
  /** Where a problem with this host is kept: github.com and an Enterprise host each have their own sign-in. */
  problemKey(remote: R): string;
  /** What names the repository in what is kept per repository, whatever form its remote URL had. */
  repoKey(remote: R): string;
  /** How its account signs in, for a forge that works through a CLI. A forge that takes a token has none: its badge opens Settings. */
  readonly login?: Login<R>;
  /** The PR Claude Code recorded for a session, for a forge whose pane links come from the transcript. */
  fromSession?(sessionId: string): Promise<L | null>;
  /** The PR for a branch, or null. */
  find(cwd: string, remote: R, branch: string): Promise<L | null>;
  /** The signed-in account, which a comment's `by` is matched against. Lookups the list makes are kept, so this costs no extra call. */
  me(remote: R): Promise<Me>;
  /** The repo's open PRs. */
  list(cwd: string, remote: R): Promise<ListedPr[]>;
  /** The link a tab's args name, which may need the provider asked again. */
  linkOf(args: PrArgs): Promise<L>;
  /** `fresh` is a read the user asked for, which a forge that keeps what it listed reads again. */
  status(link: L, options?: { fresh?: boolean }): Promise<Status>;
  /** The status, author and description, for the PR tab. */
  overview(link: L): Promise<Overview>;
  /** The patch and the comments the diff viewer shows. `st` is the PR's status, whose branches or commits a forge that diffs in the project's folder reads. */
  diff(link: L, st: Status): Promise<{ patch: string; comments: Comment[] }>;
  /** The PR's conversation for the tab. */
  comments(link: L): Promise<Comment[]>;
  /** Why one failed check failed: its failed step and the end of its log. */
  failure(link: L, f: Failing): Promise<FailureDetail[]>;
  /** A check's run: its jobs and steps, where the provider's CI has them. */
  run?(link: L, ref: RunRef): Promise<Run>;
  /** One step's log as the CI wrote it, colours and markup kept. */
  runLog?(link: L, ref: RunRef, step: RunStep): Promise<string[]>;
  /** Posts a reply to a thread. */
  reply(link: L, root: Comment, body: string): Promise<Comment>;
  /** Approves as you. `sha` is the head commit you were looking at, which GitLab approves only. Absent where the provider votes instead. */
  approve?(link: L, sha: string): Promise<void>;
  /** Casts an Azure DevOps vote. */
  vote?(link: L, vote: AzureVote): Promise<void>;
  /** The PR's pushes with their commits, where the provider lists them. */
  pushes?(link: L): Promise<Push[]>;
  /** The lines around a code thread's comment, from the commit it was written on, or with `now` where they sit at the latest push. Null where they can't be had. */
  snippet?(link: L, root: Comment, pushes: Push[], head: string | null, now?: boolean): Promise<SnippetLine[] | null>;
  /** Forgets what it kept about the account, like who you are, after a sign-in or Try again. */
  forget?(): void;
}
