export type Remote = { kind: "azure"; org: string } | { kind: "none"; org?: undefined };

export async function git(cwd: string, args: string[]) {
  const out = await wings.exec("git", args, { cwd });
  if (out.code !== 0) throw new Error(out.stderr.trim() || `git ${args[0]} failed`);
  return out.stdout.trim();
}

/** Azure remotes: ssh v3, https dev.azure.com, and the older {org}.visualstudio.com form. */
export function parseRemote(url: string): Remote {
  let m = url.match(/ssh\.dev\.azure\.com:v3\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/);
  if (!m) m = url.match(/dev\.azure\.com\/([^/]+)\/([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://dev.azure.com/${m[1]}` };
  m = url.match(/([^/.@]+)\.visualstudio\.com\/(?:DefaultCollection\/)?([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://${m[1]}.visualstudio.com` };
  return { kind: "none" };
}

/** The first run of 3+ digits that is a whole path segment or followed by `-`: `feature/1234-login` → 1234. */
export function workItemId(branch: string) {
  return branch.match(/(?:^|[/_-])(\d{3,})(?=-|\/|$)/)?.[1] ?? null;
}
