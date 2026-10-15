// Reading git from a plugin: the helpers each plugin had its own copy of, with one rule for error text and trimming.
// A plugin still lists each git command it runs in `permissions.exec`.
import { cached } from "./async.js";

/** Runs `git args` in `cwd` and resolves what `wings.exec` does: the exit code, stdout and stderr, exactly as git gave them. */
export const gitRun = (cwd, args) => globalThis.wings.exec("git", args, { cwd });

// ssh prints warnings, like OpenSSH's post-quantum notice, as "** " lines before the real error.
const errorLine = (stderr) => stderr.split("\n").find((line) => line.trim() && !line.startsWith("** "))?.trim();

/**
 * Runs `git args` in `cwd` and resolves its stdout with the whitespace around it trimmed. `raw` keeps it exactly as git
 * printed it, for a patch or a `-z` list. A non-zero exit throws git's first line of stderr, or `git <command> failed`.
 */
export async function git(cwd, args, { raw = false } = {}) {
  const r = await gitRun(cwd, args);
  if (r.code !== 0) throw new Error(errorLine(r.stderr) || `git ${args[0]} failed`);
  return raw ? r.stdout : r.stdout.trim();
}

/**
 * Runs `git args` in `cwd` and resolves its trimmed stdout, or null when git exits non-zero or `wings.exec` rejects
 * (git refused the call, or the plugin isn't approved yet). It never rejects, so nothing caches a rejection.
 */
export const gitOk = (cwd, args) =>
  gitRun(cwd, args).then(
    (r) => (r.code === 0 ? r.stdout.trim() : null),
    () => null,
  );

const NONE = new Error("nothing found");
/** A lookup that answers null for "nothing", kept per argument. Only an answer is kept, so a change later on is seen. */
function keepFound(look) {
  const found = cached(async (arg) => (await look(arg)) || Promise.reject(NONE));
  return (arg) => found.get(arg).catch(() => null);
}

/** The top folder of the repo that `cwd` is in, or null outside one. Found roots are kept; a miss is asked again next time. */
export const repoRoot = keepFound((cwd) => cwd && gitOk(cwd, ["rev-parse", "--show-toplevel"]));

/** The branch a repo's "this branch" is compared against: `origin/HEAD` as the remote set it, else `origin/main`, `origin/master`, `main` or `master`. Null when none exists. Kept once found. */
export const defaultBranch = keepFound(async (root) => {
  const head = await gitOk(root, ["rev-parse", "--abbrev-ref", "origin/HEAD"]);
  if (head && head !== "origin/HEAD") return head;
  for (const name of ["origin/main", "origin/master", "main", "master"]) {
    if (await gitOk(root, ["rev-parse", "--verify", "--quiet", `${name}^{commit}`])) return name;
  }
  return null;
});

/** What to call a checkout: its branch, else "detached at" and its short commit, else "no commits". `head` is `git rev-parse --verify --quiet HEAD`, null in a repo with no commits. */
export const branchLabel = (branch, head) => branch || (head ? `detached at ${head.slice(0, 7)}` : "no commits");

/** Hosts of Gitea and Forgejo that are known by name. */
const GITEA_HOSTS = ["codeberg.org"];
/** Bitbucket workspace and repository slugs, which the API takes in the path as they are. */
const SLUG = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/**
 * The host and repository path of an https, ssh or git URL, or scp-style `user@host:path`. A port is dropped: an ssh
 * port is the git server's, not the web server's that the CLIs and APIs talk to.
 */
function splitRemote(text) {
  // `scheme://[user@]host[:port]/path`, or scp-style `[user@]host:path`, where what follows the colon is a path even when it starts with digits.
  const m = text.match(/^[a-z][a-z0-9+.-]*:\/\//i) ? text.match(/^([a-z][a-z0-9+.-]*):\/\/(?:[^@/]+@)?(\[[^\]]+\]|[^/:]+)(?::\d+)?\/(.+)$/i) : text.match(/^(?:[^@/:]+@)?()([^/:]+):(?!\/)(.+)$/);
  if (!m) return null;
  const [, scheme, name, path] = m;
  if (scheme?.toLowerCase() === "file" || !/^(\[.+\]|[a-z0-9]([a-z0-9.-]*[a-z0-9])?)$/i.test(name)) return null;
  return { host: name.toLowerCase(), path: path.replace(/(?:\.git)?\/*$/, "") };
}

/**
 * Which forge a remote URL points at, from its form: Azure DevOps (ssh, dev.azure.com and visualstudio.com), github.com,
 * gitlab.com and hosts with "gitlab" in their name, Bitbucket Cloud and Codeberg. Any other host is `unknown`, and
 * what isn't a remote URL is `none`.
 */
export function parseRemote(url) {
  const text = url.trim();
  let m = text.match(/ssh\.dev\.azure\.com:v3\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/);
  if (!m) m = text.match(/dev\.azure\.com\/([^/]+)\/([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://dev.azure.com/${m[1]}`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  m = text.match(/([^/.@]+)\.visualstudio\.com\/(?:DefaultCollection\/)?([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://${m[1]}.visualstudio.com`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  const parts = splitRemote(text);
  if (!parts) return { kind: "none" };
  const { host, path } = parts;
  // ssh.github.com on port 443 is github.com's ssh over the https port.
  if (host === "github.com" || host === "ssh.github.com") return { kind: "github", host: "github.com", path };
  const segments = path.split("/");
  if (host === "bitbucket.org") {
    return segments.length === 2 && segments.every((s) => SLUG.test(s)) ? { kind: "bitbucket", workspace: segments[0], repo: segments[1] } : { kind: "none", host };
  }
  if (GITEA_HOSTS.includes(host)) return segments.length === 2 ? { kind: "gitea", host, owner: segments[0], repo: segments[1] } : { kind: "none", host };
  // gitlab.com, or a self-managed server with "gitlab" in its host name.
  if (host.includes("gitlab")) return { kind: "gitlab", host, path };
  return { kind: "unknown", host, path };
}
