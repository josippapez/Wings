/// <reference path="./wings.d.ts" />
import type { Batched } from "./async.js";
import type { Cli } from "./cli.js";

export interface GithubRepo {
  owner: string;
  name: string;
}

export interface GithubPulls<T> {
  /** The repository's open PRs, newest first, up to 100, as `selection` read them. */
  pulls: T[];
  /** The numbers of those that ask for your review, you or one of your teams. */
  asked: number[];
}

/**
 * The open pull requests of GitHub repositories, read through `gh api graphql`: one call for every repository asked in
 * the same microtask turn, and in it the numbers of the PRs that ask for your review (yours or your team's), which one
 * search answers for every repository. `selection` is what to read of each PullRequest, as GraphQL text. `host` is a
 * GitHub Enterprise Server, `key` what the CLI records a sign-in problem under. `get({ owner, name })` resolves the
 * repository's PRs, newest first, up to 100, and `asked`, the numbers of the ones among them that ask for your review
 * (of the first 100 review requests over every repository). A repository that can't be read rejects its own `get`.
 */
export function githubPulls<T = Record<string, any>>(gh: Cli, options: { selection: string; host?: string; key?: string }): Batched<GithubRepo, GithubPulls<T>>;
