// What every forge's mappers share: the label a PR gets, which PR a branch has, a second page, and a failure with no
// log. No `wings`, no I/O, so node tests drive it.
import { CliError } from "wings-plugin-sdk/cli";
import type { Counts, FailureDetail, Failing, Icon, Link, Result } from "../types";

export function counts(results: Result[]): Counts {
  const c: Counts = { passed: 0, failed: 0, running: 0 };
  for (const r of results) c[r]++;
  return c;
}

type Step = [on: unknown, label: string, tone: Tone, icon?: Icon];

/** The first step that holds names the state, as its label, tone and icon; `rest` when none does. */
export function ladder(steps: Step[], rest = "In review"): [string, Tone, Icon] {
  const hit = steps.find(([on]) => on);
  return hit ? [hit[1], hit[2], hit[3] ?? "pr-open"] : [rest, "info", "pr-open"];
}

export interface Facts {
  merged?: boolean;
  closed?: boolean;
  draft?: boolean;
  conflicts?: boolean;
  /** Azure DevOps: a reviewer rejected it. */
  rejected?: boolean;
  checks: { failed: number; running: number };
  /** A reviewer asked for changes. */
  changes?: boolean;
  /** GitLab: discussions are still to resolve. */
  unresolved?: boolean;
  approved?: boolean;
  /** GitHub: a review is required and hasn't been given. */
  reviewing?: boolean;
}

/** What a provider calls the states it words its own way, and what an open PR with nothing to say is called. */
export interface Words {
  closed?: string;
  changes?: string;
  rest?: string;
}

/** The label, tone and icon every forge's PR goes through, in the order they apply; what is true first wins. */
export function labelOf(f: Facts, words: Words = {}): [string, Tone, Icon] {
  return ladder(
    [
      [f.merged, "Merged", "merged", "pr-merged"],
      [f.closed, words.closed ?? "Closed", "neutral", "pr-closed"],
      [f.draft, "Draft", "neutral", "pr-draft"],
      [f.conflicts, "Conflicts", "danger"],
      [f.rejected, "Rejected", "danger"],
      [f.checks.failed, "Checks failing", "danger"],
      [f.changes, words.changes ?? "Changes requested", "danger"],
      [f.checks.running, "Checks running", "warning"],
      [f.unresolved, "Unresolved threads", "warning"],
      [f.approved, "Approved", "success"],
      [f.reviewing, "In review", "info"],
    ],
    words.rest,
  );
}

/** Whether a login or name from a list is the user's, which providers spell with any case. */
export const same = (a: string | undefined | null, b: string) => Boolean(a) && a!.toLowerCase() === b.toLowerCase();

/** The open PR for a branch wins; otherwise the newest, which a provider lists first. */
export const openOrNewest = <T>(prs: T[], isOpen: (pr: T) => boolean): T | undefined => prs.find(isOpen) ?? prs[0];

/** A list call answers a page of `size`. A repo with more gets the second page too, which is as far as a list goes. */
export async function withSecondPage<T>(first: T[], size: number, second: () => Promise<T[]>): Promise<T[]> {
  return first.length === size ? [...first, ...(await second())] : first;
}

/** A failed check with no log to read: what it says, and where the CI shows it. */
export const noLog = (check: string | undefined, summary: string, url: string | null | undefined): FailureDetail[] => [{ check, summary, log: "", url }];

/** The failure of a check on a forge whose CI has no log to fetch, only the summary and a link to the CI's own page. */
export const plainFailure = (_link: Link, f: Failing): Promise<FailureDetail[]> => Promise.resolve(noLog(f.name, f.summary ?? "Failed", f.url));

/** A read whose failure only costs what it adds: `fallback` when its CLI ran and failed. A CLI that isn't installed still throws. */
export const orElse = <T>(read: Promise<T>, fallback: T): Promise<T> =>
  read.catch((error: unknown) => {
    if (error instanceof CliError && error.kind !== "missing") return fallback;
    throw error;
  });

/**
 * A value in a query string. `wings.fetch` takes only a URL already in normal form, which leaves `%XX` as written
 * but would rewrite a bare `'`, so everything but the unreserved characters is escaped.
 */
export function encodeQuery(value: string) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
}
