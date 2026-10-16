// Running az, gh or glab from a plugin: tells "not installed", "signed out" and a plain failure apart, and keeps what a
// pane badge needs to offer Install or Sign in. The plugin still lists each command in `permissions.exec`.
import { errorMessage } from "./async.js";

/** How `wings.exec` rejects for a program that isn't installed. */
const NOT_INSTALLED = /is not installed or not on PATH/;
const NAMES = { az: "Azure CLI", gh: "GitHub CLI", glab: "GitLab CLI" };
/** The longest `wings.exec` allows, for a sign-in that waits on a browser. */
const SIGN_IN_TIMEOUT_MS = 300_000;
/** The last line of `text`, which is the error az, gh and glab print after their warnings and context. */
export const lastLine = (text) => text.trim().split("\n").at(-1);

/** What `run` and `json` throw. `kind` says why, so a plugin doesn't match on text. `stderr` is all of it, when the program ran. */
export class CliError extends Error {
  constructor(kind, message, stderr = "") {
    super(message);
    this.name = "CliError";
    this.kind = kind;
    this.stderr = stderr;
  }
}

/**
 * A handle on one CLI. `run` and `json` call it and throw a `CliError`: `missing` when it isn't installed, `signed-out`
 * when `signedOut` says so, else `failed`, with all of stderr, trimmed, as the message (`lastLine` takes its last line, for a badge). Unless `quiet`, a call
 * records `missing` and `signed-out` under its `key` (the Azure organization, or a provider and host), and a call
 * that works clears it, so `problem` and `problemBadge` say what a pane should offer. A background read that must
 * not turn badges into sign-in prompts is `quiet`. `signedOut` is a RegExp tested on stderr, or a function of the
 * result and the key that answers true or false, for a signal a regexp can't see. `signIn` is the arguments of the
 * program's login, or a function of the key that gives them. `sticky` keeps a recorded problem until `retry`, even when
 * a later call works, for a host that answers some calls without sign-in. `wings` is for tests.
 */
export function cli(program, { signedOut, signIn, sticky = false, wings }) {
  // Looked up per call: a page may create its helper at import, before a test or screenshot stub installs `wings`.
  const w = () => wings ?? globalThis.wings;
  /** key → { kind, message } */
  const problems = new Map();
  const changeListeners = new Set();
  const retryListeners = new Set();
  /** `{ key }` while a sign-in runs. */
  let signing = null;
  const changed = () => changeListeners.forEach((fn) => fn());
  const forget = (key) => problems.delete(key) && changed();
  const record = (key, problem) => {
    const known = problems.get(key);
    problems.set(key, problem);
    if (known?.kind !== problem.kind || known.message !== problem.message) changed();
  };

  async function run(args, { key, cwd, timeoutMs, quiet = false } = {}) {
    let out;
    try {
      out = await w().exec(program, args, { cwd, timeoutMs });
    } catch (error) {
      const message = errorMessage(error);
      const missing = NOT_INSTALLED.test(message);
      if (missing && !quiet) record(key, { kind: "missing", message });
      throw new CliError(missing ? "missing" : "failed", message);
    }
    if (out.code === 0) {
      if (!quiet && !sticky) forget(key);
      return out.stdout;
    }
    const text = out.stderr.trim();
    const loggedOut = typeof signedOut === "function" ? await signedOut(out, key) : signedOut.test(out.stderr);
    if (loggedOut && !quiet) record(key, { kind: "signed-out", message: lastLine(text) || `${program} failed` });
    throw new CliError(loggedOut ? "signed-out" : "failed", text || `${program} failed`, out.stderr);
  }

  async function retry() {
    problems.clear();
    try {
      await Promise.all([...retryListeners].map((fn) => fn()));
    } finally {
      changed();
    }
  }

  return {
    run,
    /**
     * `run`, parsed as JSON. A program that printed nothing, like `az boards query` for no rows, gives the call's `empty`;
     * without one that is a `failed` CliError, so a result is never null.
     */
    async json(args, call = {}) {
      const text = await run(args, call);
      if (text.trim()) return JSON.parse(text);
      if ("empty" in call) return call.empty;
      throw new CliError("failed", `${program} printed nothing`);
    },
    problem: (key) => problems.get(key) ?? null,
    problemBadge(key, { title, signedOut: signedOutText, missing: missingText, tone = "warning", icon }) {
      const problem = problems.get(key);
      if (!problem) return null;
      const badge = { tone, ...(icon ? { icon } : {}), title };
      const retryAction = { id: "retry", label: "Try again" };
      if (problem.kind === "missing") {
        return { ...badge, label: `Install ${NAMES[program]}`, subtitle: missingText, actions: [{ id: "install", label: "Install", primary: true }, retryAction] };
      }
      const busy = signing !== null && (signing.key === undefined || signing.key === key);
      return {
        ...badge,
        label: busy ? "Signing in…" : `Sign in to ${title}`,
        loading: busy,
        subtitle: busy ? "Finish signing in in your browser." : signedOutText,
        actions: [{ id: "signin", label: "Sign in", primary: true }, retryAction],
      };
    },
    /**
     * Runs the program's login, for up to 5 minutes, then `retry`. `onOutput` gets each line it prints, for a login that
     * prints a code to show. One runs at a time: a call while another runs does nothing. A badge for `key` shows
     * "Signing in…" meanwhile, and for any key when there's none.
     */
    async signIn(key, { onOutput } = {}) {
      if (signing) return;
      signing = { key };
      changed();
      try {
        const args = typeof signIn === "function" ? signIn(key) : signIn;
        const out = await w().exec(program, args, { timeoutMs: SIGN_IN_TIMEOUT_MS, onOutput });
        if (out.code !== 0) throw new Error(lastLine(out.stderr) || "Sign-in failed");
      } catch (error) {
        signing = null;
        changed();
        throw error;
      }
      signing = null;
      return retry();
    },
    /** Asks Wings to install the program. True once the install started or it's there, false if the user cancelled. */
    install: () => w().installTool(program),
    retry,
    onRetry(listener) {
      retryListeners.add(listener);
      return () => retryListeners.delete(listener);
    },
    onChange(listener) {
      changeListeners.add(listener);
      return () => changeListeners.delete(listener);
    },
  };
}
