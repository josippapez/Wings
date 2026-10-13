// Reading a git remote URL into the repository it names. No `wings`, no I/O, so node tests drive it.
import type { Remote } from "./types";

/** Hosts of Gitea and Forgejo that the plugin's `fetch` permission reaches. */
export const GITEA_HOSTS = ["codeberg.org"];

/** Bitbucket workspace and repository slugs, which the API takes in the path as they are. */
const SLUG = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/**
 * The host and repository path of an https, ssh or git URL, or scp-style `user@host:path`. A port is dropped: an ssh
 * port is the git server's, not the web server's the CLIs and APIs talk to.
 */
export function splitRemote(url: string): { host: string; path: string } | null {
  const text = url.trim();
  // `scheme://[user@]host[:port]/path`, or scp-style `[user@]host:path`, where what follows the colon is a path even when it starts with digits.
  const m = text.match(/^[a-z][a-z0-9+.-]*:\/\//i) ? text.match(/^([a-z][a-z0-9+.-]*):\/\/(?:[^@/]+@)?(\[[^\]]+\]|[^/:]+)(?::\d+)?\/(.+)$/i) : text.match(/^(?:[^@/:]+@)?()([^/:]+):(?!\/)(.+)$/);
  if (!m) return null;
  const [, scheme, name, path] = m;
  if (scheme?.toLowerCase() === "file" || !/^(\[.+\]|[a-z0-9]([a-z0-9.-]*[a-z0-9])?)$/i.test(name)) return null;
  return { host: name.toLowerCase(), path: path.replace(/(?:\.git)?\/*$/, "") };
}

/** Which provider a remote URL points at, from its form: Azure DevOps, github.com, gitlab.com and hosts named like them, Bitbucket Cloud and Codeberg. Any other host is `unknown`. */
export function parseRemote(url: string): Remote {
  let m = url.match(/ssh\.dev\.azure\.com:v3\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/);
  if (!m) m = url.match(/dev\.azure\.com\/([^/]+)\/([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://dev.azure.com/${m[1]}`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  m = url.match(/([^/.@]+)\.visualstudio\.com\/(?:DefaultCollection\/)?([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://${m[1]}.visualstudio.com`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  const parts = splitRemote(url);
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
