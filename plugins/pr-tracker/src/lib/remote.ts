// ---------- repository detection ----------
import { git } from "./cli";
import { remotes } from "./state";
import type { Remote } from "./types";

/** Azure remotes: ssh v3, https dev.azure.com, and the older {org}.visualstudio.com form. */
export function parseRemote(url: string): Remote {
  if (/github\.com[:/]/.test(url)) return { kind: "github" };
  let m = url.match(/ssh\.dev\.azure\.com:v3\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/);
  if (!m) m = url.match(/dev\.azure\.com\/([^/]+)\/([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://dev.azure.com/${m[1]}`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  m = url.match(/([^/.@]+)\.visualstudio\.com\/(?:DefaultCollection\/)?([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://${m[1]}.visualstudio.com`, project: decodeURIComponent(m[2]), repo: decodeURIComponent(m[3]) };
  // gitlab.com, or a self-managed server with "gitlab" in its host name; ssh, scp-style or https.
  m = url.match(/^(?:[a-z+]+:\/\/)?(?:[^@/]+@)?([^/:]*gitlab[^/:]*)(?::\d+)?[:/](.+?)(?:\.git)?\/?$/i);
  if (m) return { kind: "gitlab", host: m[1].toLowerCase(), path: m[2] };
  return { kind: "none" };
}

export async function remoteFor(cwd: string) {
  if (!remotes.has(cwd)) {
    try {
      const origin = await git(cwd, ["remote", "get-url", "origin"]);
      remotes.set(cwd, { ...parseRemote(origin), origin });
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  return remotes.get(cwd)!;
}
