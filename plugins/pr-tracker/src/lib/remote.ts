// ---------- repository detection ----------
import { git } from "./cli";
import { parseRemote } from "./parse";
import { remotes } from "./state";
import type { KnownRemote, UnknownRemote } from "./types";

export { parseRemote };

const HOSTS_TTL_MS = 60_000;
/** How long a remote that no CLI knows stays unresolved without asking the CLIs again, unless Try again or a sign-in in Wings forgets it. */
const UNRESOLVED_TTL_MS = 30 * 60_000;

/** origin → when to look again. */
const unresolved = new Map<string, number>();

/** The hosts `gh` and `glab` are set up for, which is how a host with no telling name (`git.corp.example`) is known to be GitHub or GitLab. */
let signedIn: { at: number; hosts: Promise<{ github: Set<string>; gitlab: Set<string> }> } | null = null;

/** `gh auth status --json hosts` prints `{"hosts":{"<host>":[...]}}` and exits 0 whatever the state; glab names each host on a line of its own, on stderr. */
async function readHosts() {
  const github = new Set<string>();
  const gitlab = new Set<string>();
  const [gh, glab] = await Promise.all([
    wings.exec("gh", ["auth", "status", "--json", "hosts"]).catch(() => null),
    wings.exec("glab", ["auth", "status", "--all"]).catch(() => null),
  ]);
  try {
    for (const host of Object.keys((JSON.parse(gh?.stdout ?? "{}") as { hosts?: Record<string, unknown> }).hosts ?? {})) github.add(host.toLowerCase());
  } catch {
    // Not JSON: an old gh without --json knows no hosts this way.
  }
  for (const line of `${glab?.stdout ?? ""}\n${glab?.stderr ?? ""}`.replace(/\x1b\[[0-9;]*m/g, "").split("\n")) {
    if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(:\d+)?$/i.test(line)) gitlab.add(line.toLowerCase());
  }
  return { github, gitlab };
}

/** Forgets which hosts the CLIs know, after signing in or Try again. */
export function forgetHosts() {
  signedIn = null;
  unresolved.clear();
}

async function resolve(remote: UnknownRemote): Promise<KnownRemote> {
  if (!signedIn || Date.now() - signedIn.at > HOSTS_TTL_MS) signedIn = { at: Date.now(), hosts: readHosts() };
  const { github, gitlab } = await signedIn.hosts;
  const { host, path, origin } = remote;
  if (github.has(host)) return { kind: "github", host, path, origin };
  if (gitlab.has(host)) return { kind: "gitlab", host, path, origin };
  return { kind: "none", host, origin };
}

export async function remoteFor(cwd: string): Promise<KnownRemote> {
  if (!remotes.has(cwd)) {
    try {
      const origin = await git(cwd, ["remote", "get-url", "origin"]);
      remotes.set(cwd, { ...parseRemote(origin), origin });
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  const remote = remotes.get(cwd)!;
  if (remote.kind !== "unknown") return remote;
  const until = unresolved.get(remote.origin!);
  if (until && Date.now() < until) return { kind: "none", host: remote.host, origin: remote.origin };
  const resolved = await resolve(remote);
  if (resolved.kind === "none") unresolved.set(remote.origin!, Date.now() + UNRESOLVED_TTL_MS);
  else remotes.set(cwd, resolved);
  return resolved;
}
