// ---------- repository detection ----------
import { gitOk, parseRemote } from "wings-plugin-sdk/git";
import type { GithubRemote, GitlabRemote, KnownRemote, Remote } from "../lib/types";

const HOSTS_TTL_MS = 60_000;
/** How long a remote that no CLI knows stays unresolved without asking the CLIs again, unless Try again or a sign-in in Wings forgets it. */
const UNRESOLVED_TTL_MS = 30 * 60_000;

/** cwd → what its `origin` is, with a host no provider's name gives away replaced once `gh` or `glab` is known to be signed in to it. */
export const remotes = new Map<string, Remote>();

/** `host/path` → when to look again. */
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

async function resolve({ host, path }: { host: string; path: string }): Promise<GithubRemote | GitlabRemote | { kind: "none"; host: string }> {
  if (!signedIn || Date.now() - signedIn.at > HOSTS_TTL_MS) signedIn = { at: Date.now(), hosts: readHosts() };
  const { github, gitlab } = await signedIn.hosts;
  if (github.has(host)) return { kind: "github", host, path };
  if (gitlab.has(host)) return { kind: "gitlab", host, path };
  return { kind: "none", host };
}

/** The remote of a folder's `origin`. A host with no telling name is `none` until `gh` or `glab` is signed in to it, which is asked again after a while. */
export async function remoteFor(cwd: string): Promise<KnownRemote | { kind: "none"; host?: string }> {
  if (!remotes.has(cwd)) remotes.set(cwd, parseRemote((await gitOk(cwd, ["remote", "get-url", "origin"])) ?? ""));
  const remote = remotes.get(cwd)!;
  if (remote.kind !== "unknown") return remote;
  const key = `${remote.host}/${remote.path}`;
  if ((unresolved.get(key) ?? 0) > Date.now()) return { kind: "none", host: remote.host };
  const resolved = await resolve(remote);
  if (resolved.kind === "none") unresolved.set(key, Date.now() + UNRESOLVED_TTL_MS);
  else remotes.set(cwd, resolved);
  return resolved;
}
