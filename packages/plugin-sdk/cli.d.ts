/// <reference path="./wings.d.ts" />
export type CliKind = "missing" | "signed-out" | "failed";

/** The last line of `text`, which is the error az, gh and glab print after their warnings and context. For a badge, which has room for one line. */
export function lastLine(text: string): string;

/** What `run` and `json` throw. `kind` says why, so a plugin doesn't match on text. `message` is all of stderr, trimmed, when the program ran and said something. `stderr` is it untouched. */
export class CliError extends Error {
  constructor(kind: CliKind, message: string, stderr?: string);
  kind: CliKind;
  stderr: string;
}

/** What `wings.exec` resolves. */
export interface CliResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

export interface CliOptions {
  /** A RegExp tested on stderr, or a function of the result and the key that answers true or false, for a signal a regexp can't see. */
  signedOut: RegExp | ((result: CliResult, key: string | undefined) => boolean | Promise<boolean>);
  /** The arguments of the program's login, like `["login", "--allow-no-subscriptions", "--output", "none"]`, or a function of the key that gives them. */
  signIn: string[] | ((key: string | undefined) => string[]);
  /** Keep a recorded problem until `retry`, even when a later call works. For a host that answers some calls without sign-in. */
  sticky?: boolean;
  /** For tests: a stand-in for the `wings` global. */
  wings?: Pick<Wings, "exec" | "installTool">;
}

export interface CliCall {
  /** What a problem is recorded under: the Azure organization, or a provider and host. */
  key?: string;
  cwd?: string;
  /** 30 s by default, 5 min at most. */
  timeoutMs?: number;
  /** Record no problem and clear none, for a background read that must not turn badges into sign-in prompts. */
  quiet?: boolean;
}

export interface CliProblem {
  kind: "missing" | "signed-out";
  /** The program's own message: the last line of stderr, or what `wings.exec` rejected with. */
  message: string;
}

/** What a problem badge says. `title` is the service's name, like "Azure DevOps". */
export interface CliBadgeText {
  title: string;
  /** Under the Sign in button. */
  signedOut: string;
  /** Under the Install button. */
  missing: string;
  /** `warning` when unset. */
  tone?: Tone;
  icon?: WingsBadge["icon"];
}

export interface Cli {
  /** Runs the program with `args` and resolves its stdout. Throws a `CliError`. */
  run(args: string[], call?: CliCall): Promise<string>;
  /**
   * Runs the program like `wings.exec` and resolves `{ code, stdout, stderr }` whatever the exit code, for a command
   * whose output matters when it fails, like `gh auth status`. Records and clears no sign-in problem. It throws a
   * `CliError` only when the program didn't run: `missing` when it isn't installed (recorded unless `quiet`), else `failed`.
   */
  exec(args: string[], call?: CliCall): Promise<CliResult>;
  /**
   * `run`, parsed as JSON. A program that printed nothing, like `az boards query` for no rows, gives the call's `empty`;
   * without one that is a `failed` CliError, so a result is never null.
   */
  json<T = unknown>(args: string[], call?: CliCall & { empty?: T }): Promise<T>;
  /** What the last non-quiet call under `key` found, until a call under it works or `retry`. */
  problem(key?: string): CliProblem | null;
  /**
   * The badge to show a pane while `key` has a problem, with the actions `install` or `signin` (primary) and `retry`, or
   * null without one. While a sign-in runs it says "Signing in…" and spins.
   */
  problemBadge(key: string | undefined, text: CliBadgeText): WingsBadge | null;
  /**
   * Runs the program's login, for up to 5 minutes, then `retry`. `onOutput` gets each line it prints, for a login that
   * prints a code to show. One runs at a time: a call while another runs does nothing. A badge for `key` shows
   * "Signing in…" meanwhile, and for any key when there's none.
   */
  signIn(key?: string, options?: { onOutput?: (line: string) => void }): Promise<void>;
  /** Asks Wings to install the program. True once the install started or it's there, false if the user cancelled. */
  install(): Promise<boolean>;
  /** Forgets every problem, runs the `onRetry` listeners and waits for them, then tells `onChange`. */
  retry(): Promise<void>;
  /** For a plugin to clear its caches and load again when the user tries again or has signed in. */
  onRetry(listener: () => unknown): () => void;
  /** Called when a problem is recorded, changes or goes, or a sign-in starts or ends: the plugin draws its badges again. */
  onChange(listener: () => void): () => void;
}

/**
 * A handle on one CLI. `run` and `json` call it and throw a `CliError`: `missing` when it isn't installed, `signed-out`
 * when `signedOut` says so, else `failed`, with all of stderr, trimmed, as the message (`lastLine` takes its last line, for a badge). Unless `quiet`, a call
 * records `missing` and `signed-out` under its `key` (the Azure organization, or a provider and host), and a call
 * that works clears it, so `problem` and `problemBadge` say what a pane should offer.
 */
export function cli(program: "az" | "gh" | "glab", options: CliOptions): Cli;
