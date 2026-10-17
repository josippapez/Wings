/** What the main script and the pages say to each other, declared once so each side's calls are checked against the same shapes. */
import { channel } from "wings-plugin-sdk/messages";
import type { PullRow } from "./pulls";
import type { Push } from "./pushes";
import type { SnippetLine } from "./snippet";
import type { Comment, Failing, FailureDetail, Overview, PrArgs, Run, RunRef, RunStep } from "./types";
import type { AzureVote } from "./vote";

/** What a block or a tab shows when the host isn't set up, and the label of the button that sets it up. */
export interface Problem {
  message: string;
  action: "Sign in" | "Add token" | "Install";
}

/** What main answers a block with. `none` is a project with no remote of a provider the tracker knows. */
export interface PullsAnswer {
  rows?: PullRow[];
  none?: true;
  /** With `none`: the host nothing recognised, which Check again looks up once more. */
  host?: string;
  problem?: Problem;
}

/** A tab's PR, as its queries are kept: its args without the title and the project, so tabs opened from anywhere share one copy. */
export type PrKey = Omit<PrArgs, "title" | "cwd">;

/** What a PR tab shows at the top: the overview, or the problem with the host instead, and what the PR's forge can do. */
export interface PrValue {
  overview?: Overview;
  problem?: Problem;
  can: { approve: boolean; vote: boolean; snippet: boolean };
}

export const ch = channel<
  {
    /**
     * A "pr" tab asks for what only main can do: open the diff viewer that takes replies, find the agent on the branch,
     * mark the PR read, approve it at the head commit `sha` it showed, cast an Azure DevOps `vote`, or open one `commit`'s changes.
     */
    pr: { request: { op: "diff" | "agent" | "seen" | "approve" | "vote" | "commit"; args: PrArgs; focus?: WingsDiffFocus; sha?: string; vote?: AzureVote; commit?: { sha: string; message: string } }; response: void };
    /** Posts a reply to a thread, which every tab on the PR then shows. */
    "pr-reply": { request: { args: PrArgs; root: Comment; body: string }; response: Comment };
    /** The lines around a code thread's comment, from the commit it was written on, or with `now` from the latest push. */
    "pr-snippet": { request: { args: PrArgs; root: Comment; head: string | null; now?: boolean }; response: SnippetLine[] | null };
    /** Why one failed check failed. */
    "pr-failure": { request: { args: PrArgs; failing: Failing }; response: FailureDetail[] };
    /** A check's run: its jobs and steps. */
    "pr-run": { request: { args: PrArgs; run: RunRef }; response: Run };
    /** One step's log, as the CI wrote it. */
    "pr-run-log": { request: { args: PrArgs; run: RunRef; step: RunStep }; response: string[] };
    /** A block's or a tab's button: what the pane badge's Sign in, Add token or Install does for this project's host. */
    "pulls-action": { request: { path: string }; response: void };
  },
  {
    /** Settings saved or removed a token, which main asks again with. */
    "tokens-changed": void;
  },
  {
    /** A block's project's open pull requests, from the same cache as the sidebar section. */
    pulls: { key: { path: string }; value: PullsAnswer };
    /** A tab's PR: its overview, which also keeps a pane's badge on it fresh. */
    pr: { key: PrKey; value: PrValue };
    /** A tab's PR's conversation. */
    "pr-comments": { key: PrKey; value: Comment[] };
    /** A tab's PR's pushes with their commits, where the provider lists them. */
    "pr-pushes": { key: PrKey; value: Push[] };
  }
>();
