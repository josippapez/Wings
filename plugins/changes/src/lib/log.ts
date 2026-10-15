/**
 * `git log -z` fields, each ended by a NUL: hash, parents, author, author time, committer time, refs, subject.
 * `--decorate=full` so a `log.decorate` setting can't change the ref names, and local, remote and tag refs tell apart.
 */
export const LOG = ["-z", "--no-color", "--decorate=full", "--format=%H%x00%P%x00%an%x00%at%x00%ct%x00%D%x00%s"];
const LOG_FIELDS = 7;

/**
 * `refs` are `kind:name`, where kind is `H` (HEAD, on that branch if it has a name), `b` local branch, `r` remote branch, `t` tag or `s` stash.
 */
export interface Commit {
  hash: string;
  parents: string[];
  author: string;
  at: number;
  ct: number;
  refs: string[];
  subject: string;
}

/** `%D` with `--decorate=full`, as short `kind:name` refs. `origin/HEAD` and the like are left out. */
function parseRefs(decoration: string) {
  const out: string[] = [];
  for (let ref of decoration ? decoration.split(", ") : []) {
    if (ref === "HEAD") {
      out.push("H:HEAD");
      continue;
    }
    const head = ref.startsWith("HEAD -> ");
    if (head) ref = ref.slice(8);
    if (ref.startsWith("tag: ")) ref = ref.slice(5);
    if (ref.startsWith("refs/heads/")) out.push(`${head ? "H" : "b"}:${ref.slice(11)}`);
    else if (ref.startsWith("refs/remotes/") && !ref.endsWith("/HEAD")) out.push(`r:${ref.slice(13)}`);
    else if (ref.startsWith("refs/tags/")) out.push(`t:${ref.slice(10)}`);
    else if (ref === "refs/stash") out.push("s:stash");
  }
  return out;
}

const REF_KIND: Record<string, string> = { H: "HEAD", b: "Branch", r: "Remote branch", t: "Tag", s: "Stash" };
const REF_TONE: Record<string, "info" | "success" | "warn" | undefined> = { H: "info", b: "success", t: "warn" };

/** How a `kind:name` ref shows as a pill, in the History list and in the graph. */
export function describeRef(ref: string) {
  const kind = ref[0];
  const name = ref.slice(2);
  return {
    head: kind === "H",
    label: kind === "t" ? `◆ ${name}` : name,
    title: kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${REF_KIND[kind] ?? "Ref"} ${name}`,
    tone: REF_TONE[kind],
  };
}

/** @param out `git log` output with `LOG`. */
export function parseLog(out: string): Commit[] {
  const f = out.split("\0");
  const commits: Commit[] = [];
  for (let i = 0; i + LOG_FIELDS <= f.length; i += LOG_FIELDS) {
    const hash = f[i];
    if (!hash) break;
    commits.push({ hash, parents: f[i + 1] ? f[i + 1].split(" ") : [], author: f[i + 2], at: Number(f[i + 3]), ct: Number(f[i + 4]), refs: parseRefs(f[i + 5]), subject: f[i + 6] });
  }
  return commits;
}
