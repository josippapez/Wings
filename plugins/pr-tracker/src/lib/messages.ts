/** What the main script and the pages say to each other, declared once so each side's calls are checked against the same shapes. */
import { channel } from "wings-plugin-sdk/messages";
import type { PullRow } from "./pulls";
import type { PrArgs } from "./types";

/** What a block shows when the project's host isn't set up, and the label of the button that sets it up. */
interface PullsProblem {
  message: string;
  action: "Sign in" | "Add token" | "Install";
}

/** What main answers a block with. `none` is a project with no remote of a provider the tracker knows. */
export interface PullsAnswer {
  rows?: PullRow[];
  none?: true;
  /** With `none`: the host nothing recognised, which Check again looks up once more. */
  host?: string;
  problem?: PullsProblem;
}

export const ch = channel<
  {
    /** A "pr" tab asks for what only main can do: open the diff viewer that takes replies, find the agent on the branch, or mark the PR read. */
    pr: { request: { op: "diff" | "agent" | "seen"; args: PrArgs; focus?: WingsDiffFocus }; response: void };
    /** A block's button: what the pane badge's Sign in, Add token or Install does for this project's host. */
    "pulls-action": { request: { path: string }; response: void };
  },
  {
    /** Settings saved or removed a token, which main asks again with. */
    "tokens-changed": void;
  },
  {
    /** A block's project's open pull requests, from the same cache as the sidebar section. */
    pulls: { key: { path: string }; value: PullsAnswer };
  }
>();
