// PR tracker: shows the pull request for each terminal pane's current branch as a pane badge, with live
// status, checks, the diff and review comments. Three providers:
// - GitHub: in a Claude pane, the PR Claude Code recorded in the transcript (`pr-link`); else the PR for the branch (`gh`).
// - Azure DevOps: the PR for the branch (`az repos`), diff from local git, comments from threads.
// - GitLab: the merge request for the branch, its pipeline, approvals and discussions, all through `glab api`.

const LINK_POLL_MS = 15_000;
const LOOKUP_TTL_MS = 60_000;
const STATUS_POLL_MS = 60_000;
const SIGN_IN_TIMEOUT_MS = 300_000;
const GH_FIELDS =
  "number,title,state,isDraft,reviewDecision,mergeable,statusCheckRollup,additions,deletions,changedFiles,headRefName,baseRefName,url,updatedAt";

let panes = [];
/** cwd → { kind: "github" | "azure" | "gitlab", origin, ...provider fields } | { kind: "none" } */
const remotes = new Map();
/** paneId → PR ref { provider, url, number, repo, ...provider fields } */
const links = new Map();
/** paneId → { ahead, behind }: commits to push and pull on the pane's branch, as of the last fetch. */
const sync = new Map();
/** `${origin} ${branch}` → { at, link: Promise }, so panes on the same branch share one network lookup. */
const lookups = new Map();
/** PR url → normalized status, or { error } */
const status = new Map();
const inFlight = new Set();
/** PRs refreshed because you clicked Refresh; background polls don't show a spinner. */
const manual = new Set();
/** PR url → { key, patch, comments }, fetched ahead so "View changes" opens instantly. */
const diffs = new Map();
const diffLoads = new Map();
/** "github", an Azure org or `gitlab:<host>` → { login, message }, when its CLI can't be used there. */
const problems = new Map();
/** Provider whose sign-in is running → { code, url } once the CLI prints them. */
const signingIn = new Map();
const PROVIDERS = {
  github: { name: "GitHub", tone: "merged" },
  azure: { name: "Azure DevOps", tone: "warning" },
  gitlab: { name: "GitLab", tone: "info" },
};
const GH_LOGGED_OUT = /gh auth login|GH_TOKEN/;

wings.onPanes((list) => {
  panes = list;
  void refreshLinks();
});
setInterval(refreshLinks, LINK_POLL_MS);
setInterval(() => [...new Set([...links.values()].map((l) => l.url))].forEach((url) => refreshStatus(url)), STATUS_POLL_MS);

// ---------- repository detection ----------

async function git(cwd, args) {
  const out = await wings.exec("git", args, { cwd });
  if (out.code !== 0) throw new Error(out.stderr.trim() || `git ${args[0]} failed`);
  return out.stdout.trim();
}

/** Azure remotes: ssh v3, https dev.azure.com, and the older {org}.visualstudio.com form. */
function parseRemote(url) {
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

async function remoteFor(cwd) {
  if (!remotes.has(cwd)) {
    try {
      const origin = await git(cwd, ["remote", "get-url", "origin"]);
      remotes.set(cwd, { ...parseRemote(origin), origin });
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  return remotes.get(cwd);
}

// ---------- finding the PR for a pane ----------

async function fromTranscript(sessionId) {
  try {
    const last = (await wings.transcript(sessionId, ["pr-link"], { last: 1 })).at(-1);
    if (last?.prUrl) return { provider: "github", url: last.prUrl, number: last.prNumber, repo: last.prRepository };
  } catch {
    // A brand-new session may not have a transcript yet.
  }
  return null;
}

/** Remembers a logged-out `gh`, so the pane offers a sign-in button instead of an error. */
function checkGhLogin(out) {
  if (out.code !== 0 && GH_LOGGED_OUT.test(out.stderr)) {
    problems.set("github", { login: true, message: "Sign in to GitHub to see this branch's pull request." });
  } else if (out.code === 0) {
    problems.delete("github");
  }
}

async function findGithub(cwd, _remote, branch) {
  const out = await wings.exec("gh", ["pr", "list", "--head", branch, "--state", "all", "--limit", "1", "--json", "url,number"], { cwd });
  checkGhLogin(out);
  const pr = out.code === 0 ? JSON.parse(out.stdout)[0] : null;
  if (!pr) return null;
  const repo = pr.url.match(/github\.com\/([^/]+\/[^/]+)\/pull/)?.[1] ?? "";
  return { provider: "github", url: pr.url, number: pr.number, repo };
}

async function findAzure(cwd, remote, branch) {
  const out = await wings.exec("az", [
    "repos", "pr", "list",
    "--organization", remote.org, "--project", remote.project, "--repository", remote.repo,
    "--source-branch", branch, "--status", "all", "--top", "5", "-o", "json",
  ]);
  if (out.code !== 0) {
    const login = /login/i.test(out.stderr);
    problems.set(remote.org, { login, message: login ? "Sign in to Azure DevOps to see this branch's pull request." : out.stderr.trim() || "az failed" });
    return null;
  }
  problems.delete(remote.org);
  const prs = JSON.parse(out.stdout);
  // The open PR wins; otherwise the newest one for this branch.
  const pr = prs.find((p) => p.status === "active") ?? prs[0];
  if (!pr) return null;
  const base = pr.repository?.webUrl ?? `${remote.org}/${encodeURIComponent(remote.project)}/_git/${encodeURIComponent(remote.repo)}`;
  return { provider: "azure", url: `${base}/pullrequest/${pr.pullRequestId}`, number: pr.pullRequestId, repo: `${remote.project}/${remote.repo}`, remote, cwd, raw: pr };
}

/** `glab api` against the remote's host. Returns parsed JSON or throws, noting a signed-out or missing `glab`. */
async function glab(remote, endpoint, { paginate = false } = {}) {
  const key = `gitlab:${remote.host}`;
  let out;
  try {
    out = await wings.exec("glab", ["api", "--hostname", remote.host, ...(paginate ? ["--paginate"] : []), endpoint]);
  } catch (error) {
    problems.set(key, { login: false, message: String(error.message ?? error) });
    throw error;
  }
  if (out.code === 0) return JSON.parse(out.stdout);
  // A private project looks missing (404) to a signed-out user, so ask glab whether it's signed in.
  const signedOut =
    /\b401\b/.test(out.stderr) || (/\b404\b/.test(out.stderr) && (await wings.exec("glab", ["auth", "status", "--hostname", remote.host])).code !== 0);
  if (signedOut) problems.set(key, { login: true, message: "Sign in to GitLab to see this branch's merge request." });
  throw new Error(out.stderr.trim() || "glab failed");
}

const project = (remote) => `projects/${encodeURIComponent(remote.path)}`;

async function findGitlab(cwd, remote, branch) {
  const mrs = await glab(remote, `${project(remote)}/merge_requests?source_branch=${encodeURIComponent(branch)}&state=all&order_by=updated_at&per_page=5`);
  // The open MR wins; otherwise the newest one for this branch.
  const mr = mrs.find((m) => m.state === "opened") ?? mrs[0];
  if (!mr) return null;
  return { provider: "gitlab", url: mr.web_url, number: mr.iid, repo: remote.path, remote };
}

async function findForPane(pane) {
  const remote = await remoteFor(pane.cwd);
  if (remote.kind === "none") return null;
  if (remote.kind === "github" && pane.session) {
    const link = await fromTranscript(pane.session.sessionId);
    if (link) return link;
  }
  const branch = await git(pane.cwd, ["branch", "--show-current"]).catch(() => "");
  if (!branch) return null;
  const key = `${remote.origin} ${branch}`;
  const hit = lookups.get(key);
  if (hit && Date.now() - hit.at < LOOKUP_TTL_MS) return hit.link;
  const find = { github: findGithub, azure: findAzure, gitlab: findGitlab }[remote.kind];
  const link = find(pane.cwd, remote, branch);
  lookups.set(key, { at: Date.now(), link });
  link.catch(() => lookups.delete(key));
  return link;
}

async function refreshLinks() {
  await Promise.all(
    panes.map(async (pane) => {
      try {
        const link = await findForPane(pane);
        if (!link) return void links.delete(pane.paneId);
        const known = links.get(pane.paneId);
        links.set(pane.paneId, link);
        // Fails without an upstream branch, which means nothing to push or pull yet.
        const counts = await git(pane.cwd, ["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).catch(() => "0 0");
        const [behind, ahead] = counts.split(/\s+/).map(Number);
        sync.set(pane.paneId, { ahead, behind });
        if (known?.url !== link.url && !status.has(link.url)) void refreshStatus(link.url);
      } catch (error) {
        console.error(error);
      }
    }),
  );
  render();
}

// ---------- status ----------

function linkFor(url) {
  return [...links.values()].find((l) => l.url === url);
}

async function refreshStatus(url, { byUser = false } = {}) {
  const link = linkFor(url);
  if (!link) return;
  if (byUser) manual.add(url);
  if (inFlight.has(url)) return;
  inFlight.add(url);
  render();
  try {
    const load = { github: () => githubStatus(url), azure: () => azureChecks(link), gitlab: () => gitlabStatus(link) }[link.provider];
    status.set(url, await load());
  } catch (error) {
    status.set(url, { error: String(error.message ?? error) });
  } finally {
    inFlight.delete(url);
    manual.delete(url);
    render();
  }
  const st = status.get(url);
  if (st && !st.error && diffs.get(url)?.key !== st.key) void loadDiff(url).catch(() => {});
}

function counts(results) {
  const c = { passed: 0, failed: 0, running: 0 };
  for (const r of results) c[r]++;
  return c;
}

async function githubStatus(url) {
  const out = await wings.exec("gh", ["pr", "view", url, "--json", GH_FIELDS]);
  checkGhLogin(out);
  if (out.code !== 0) throw new Error(out.stderr.trim() || "gh failed");
  const pr = JSON.parse(out.stdout);
  const checks = counts(
    (pr.statusCheckRollup ?? []).map((c) => {
      const r = c.__typename === "StatusContext" ? c.state : c.status === "COMPLETED" ? c.conclusion : "PENDING";
      if (["SUCCESS", "NEUTRAL", "SKIPPED"].includes(r)) return "passed";
      if (["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"].includes(r)) return "failed";
      return "running";
    }),
  );
  let [label, tone, icon] = ["Open", "info", "pr-open"];
  if (pr.state === "MERGED") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (pr.state === "CLOSED") [label, tone, icon] = ["Closed", "neutral", "pr-closed"];
  else if (pr.isDraft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (pr.mergeable === "CONFLICTING") [label, tone] = ["Conflicts", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (pr.reviewDecision === "CHANGES_REQUESTED") [label, tone] = ["Changes requested", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (pr.reviewDecision === "APPROVED") [label, tone] = ["Approved", "success"];
  else if (pr.reviewDecision === "REVIEW_REQUIRED") [label, tone] = ["In review", "info"];
  const review = { APPROVED: "Approved", CHANGES_REQUESTED: "Changes requested", REVIEW_REQUIRED: "Waiting for review" }[pr.reviewDecision] ?? "No review yet";
  return {
    key: pr.updatedAt,
    title: pr.title,
    label, tone, icon, checks, review,
    changes: `+${pr.additions} −${pr.deletions} in ${pr.changedFiles} files`,
    branch: `${pr.headRefName} → ${pr.baseRefName}`,
    updated: pr.updatedAt,
  };
}

const short = (ref) => (ref ?? "").replace(/^refs\/heads\//, "");

/** Azure status from the PR plus its policy checks. Reviewer votes: 10 approved, 5 approved with suggestions, -5 waiting for author, -10 rejected. */
function azureStatus(pr, checks) {
  const votes = (pr.reviewers ?? []).map((r) => r.vote);
  let [label, tone, icon] = ["In review", "info", "pr-open"];
  if (pr.status === "completed") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (pr.status === "abandoned") [label, tone, icon] = ["Abandoned", "neutral", "pr-closed"];
  else if (pr.isDraft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (pr.mergeStatus === "conflicts") [label, tone] = ["Conflicts", "danger"];
  else if (votes.includes(-10)) [label, tone] = ["Rejected", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (votes.includes(-5)) [label, tone] = ["Waiting for author", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (votes.some((v) => v > 0)) [label, tone] = ["Approved", "success"];
  const approved = votes.filter((v) => v > 0).length;
  const waiting = votes.filter((v) => v === -5).length;
  const rejected = votes.filter((v) => v === -10).length;
  const review = [approved && `${approved} approved`, waiting && `${waiting} waiting for author`, rejected && `${rejected} rejected`].filter(Boolean).join(", ") || "No votes yet";
  return {
    // A new push changes the source commit, which is what invalidates the cached diff.
    key: pr.lastMergeSourceCommit?.commitId ?? pr.creationDate,
    title: pr.title,
    label, tone, icon, checks, review,
    changes: null,
    branch: `${short(pr.sourceRefName)} → ${short(pr.targetRefName)}`,
    updated: pr.closedDate ?? pr.creationDate,
    base: short(pr.targetRefName),
    head: short(pr.sourceRefName),
  };
}

async function azureChecks(link) {
  const out = await wings.exec("az", ["repos", "pr", "policy", "list", "--id", String(link.number), "--organization", link.remote.org, "-o", "json"]);
  const records = out.code === 0 ? JSON.parse(out.stdout) : [];
  const checks = counts(
    (Array.isArray(records) ? records : (records.value ?? []))
      .filter((r) => r.status !== "notApplicable" && r.configuration?.isEnabled !== false)
      .map((r) => (r.status === "approved" ? "passed" : r.status === "rejected" || r.status === "broken" ? "failed" : "running")),
  );
  return azureStatus(link.raw, checks);
}

/** GitLab status from the MR, its approvals and its head pipeline's jobs. */
async function gitlabStatus(link) {
  const base = `${project(link.remote)}/merge_requests/${link.number}`;
  const mr = await glab(link.remote, base);
  const pipeline = mr.head_pipeline;
  const [approvals, jobs] = await Promise.all([
    glab(link.remote, `${base}/approvals`).catch(() => null),
    pipeline ? glab(link.remote, `projects/${pipeline.project_id}/pipelines/${pipeline.id}/jobs?per_page=100`, { paginate: true }).catch(() => []) : [],
  ]);
  // Manual jobs haven't run and don't block, so they don't count. A failure that's allowed to fail passes.
  const checks = counts(
    jobs
      .filter((j) => j.status !== "manual")
      .map((j) =>
        j.status === "success" || j.status === "skipped" || (j.status === "failed" && j.allow_failure)
          ? "passed"
          : j.status === "failed" || j.status === "canceled"
            ? "failed"
            : "running",
      ),
  );
  const merge = mr.detailed_merge_status;
  let [label, tone, icon] = ["In review", "info", "pr-open"];
  if (mr.state === "merged") [label, tone, icon] = ["Merged", "merged", "pr-merged"];
  else if (mr.state === "closed" || mr.state === "locked") [label, tone, icon] = ["Closed", "neutral", "pr-closed"];
  else if (mr.draft) [label, tone, icon] = ["Draft", "neutral", "pr-draft"];
  else if (mr.has_conflicts || merge === "conflict") [label, tone] = ["Conflicts", "danger"];
  else if (checks.failed) [label, tone] = ["Checks failing", "danger"];
  else if (merge === "requested_changes") [label, tone] = ["Changes requested", "danger"];
  else if (checks.running) [label, tone] = ["Checks running", "warning"];
  else if (merge === "discussions_not_resolved") [label, tone] = ["Unresolved threads", "warning"];
  else if (approvals?.approved) [label, tone] = ["Approved", "success"];
  const approvedBy = approvals?.approved_by?.length ?? 0;
  const left = approvals?.approvals_left ?? 0;
  const review = [approvedBy && `${approvedBy} approved`, left && `${left} more needed`].filter(Boolean).join(", ") || "No approvals yet";
  return {
    // A new push changes the head commit, which is what invalidates the cached diff.
    key: mr.sha,
    title: mr.title,
    label, tone, icon, checks, review,
    changes: mr.changes_count ? `${mr.changes_count} files` : null,
    branch: `${mr.source_branch} → ${mr.target_branch}`,
    updated: mr.updated_at,
  };
}

// ---------- diff and comments ----------

/** The PR's diff and review comments, cached per PR version; concurrent callers share one fetch. */
function loadDiff(url) {
  const key = status.get(url)?.key;
  const cached = diffs.get(url);
  if (cached && cached.key === key) return Promise.resolve(cached);
  if (diffLoads.has(url)) return diffLoads.get(url);
  const link = linkFor(url);
  const load = (async () => {
    const entry = await { github: githubDiff, azure: azureDiff, gitlab: gitlabDiff }[link.provider](link);
    diffs.set(url, { key, ...entry });
    return diffs.get(url);
  })().finally(() => diffLoads.delete(url));
  diffLoads.set(url, load);
  return load;
}

async function githubDiff(link) {
  const [diff, comments, discussion] = await Promise.all([
    wings.exec("gh", ["pr", "diff", link.url]),
    wings.exec("gh", ["api", `repos/${link.repo}/pulls/${link.number}/comments`, "--paginate", "--slurp"]),
    wings.exec("gh", ["pr", "view", link.url, "--json", "comments,reviews"]),
  ]);
  if (diff.code !== 0) throw new Error(diff.stderr.trim() || "gh pr diff failed");
  const list = comments.code === 0 ? JSON.parse(comments.stdout).flat() : [];
  const pr = discussion.code === 0 ? JSON.parse(discussion.stdout) : { comments: [], reviews: [] };
  // Comments on the whole PR: conversation comments plus review summaries. Their ids are GraphQL strings,
  // so they get negative numbers that can't clash with the line comments' ids.
  const general = [
    ...pr.comments.map((c) => ({ author: c.author?.login, body: c.body, createdAt: c.createdAt, url: c.url })),
    ...pr.reviews.filter((r) => r.body).map((r) => ({ author: r.author?.login, body: r.body, createdAt: r.submittedAt, url: link.url })),
  ].map((c, i) => ({ id: -(i + 1), replyTo: null, path: null, line: null, side: "additions", ...c, author: c.author ?? "unknown" }));
  return {
    patch: diff.stdout,
    comments: [...general, ...list.map((c) => ({
      id: c.id,
      replyTo: c.in_reply_to_id ?? null,
      path: c.path,
      line: c.line ?? null,
      side: c.side === "LEFT" ? "deletions" : "additions",
      author: c.user?.login ?? "unknown",
      body: c.body ?? "",
      createdAt: c.created_at,
      url: c.html_url,
    }))],
  };
}

async function azureDiff(link) {
  const { base, head } = status.get(link.url);
  // Fetch only the two branches, then diff from their merge base, like the PR page does.
  await git(link.cwd, ["fetch", "--quiet", "origin", base, head]);
  const patch = await git(link.cwd, ["diff", "--no-color", "--no-ext-diff", `origin/${base}...origin/${head}`]);
  const threads = await wings.exec("az", [
    "devops", "invoke", "--area", "git", "--resource", "pullRequestThreads",
    "--route-parameters", `project=${link.remote.project}`, `repositoryId=${link.remote.repo}`, `pullRequestId=${link.number}`,
    "--organization", link.remote.org, "--api-version", "7.1", "-o", "json",
  ]);
  const value = threads.code === 0 ? (JSON.parse(threads.stdout).value ?? []) : [];
  const comments = [];
  for (const t of value) {
    if (t.isDeleted) continue;
    const ctx = t.threadContext;
    // Threads without a file are comments on the whole PR.
    const path = ctx?.filePath ? ctx.filePath.replace(/^\//, "") : null;
    const right = ctx?.rightFileStart?.line;
    const left = ctx?.leftFileStart?.line;
    const texts = (t.comments ?? []).filter((c) => c.commentType === "text" && !c.isDeleted);
    texts.forEach((c, i) =>
      comments.push({
        id: t.id * 10_000 + c.id,
        // Azure replies can nest; Wings shows them flat under the thread's first comment.
        replyTo: i === 0 ? null : t.id * 10_000 + texts[0].id,
        path,
        line: right ?? left ?? null,
        side: right ? "additions" : "deletions",
        author: c.author?.displayName ?? "unknown",
        body: c.content ?? "",
        createdAt: c.publishedDate,
        url: link.url,
      }),
    );
  }
  return { patch: `${patch}\n`, comments };
}

async function gitlabDiff(link) {
  const [diff, discussions] = await Promise.all([
    wings.exec("glab", ["mr", "diff", String(link.number), "--raw", "--repo", `https://${link.remote.host}/${link.remote.path}`]),
    glab(link.remote, `${project(link.remote)}/merge_requests/${link.number}/discussions?per_page=100`, { paginate: true }).catch(() => []),
  ]);
  if (diff.code !== 0) throw new Error(diff.stderr.trim() || "glab mr diff failed");
  const comments = [];
  for (const d of discussions) {
    const notes = (d.notes ?? []).filter((n) => !n.system);
    notes.forEach((n, i) => {
      // Only diff notes have a position; the rest are comments on the whole MR.
      const pos = n.type === "DiffNote" ? n.position : null;
      comments.push({
        id: n.id,
        replyTo: i === 0 ? null : notes[0].id,
        path: pos ? (pos.new_path ?? pos.old_path) : null,
        line: pos ? (pos.new_line ?? pos.old_line ?? null) : null,
        side: pos?.new_line ? "additions" : "deletions",
        author: n.author?.name ?? n.author?.username ?? "unknown",
        body: n.body ?? "",
        createdAt: n.created_at,
        url: link.url,
      });
    });
  }
  return { patch: diff.stdout, comments };
}

// ---------- badge ----------

function ago(iso) {
  const minutes = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 24 ? `${hours} h ago` : `${Math.round(hours / 24)} d ago`;
}

function syncText({ ahead, behind }) {
  return [ahead && `${ahead} to push`, behind && `${behind} to pull`].filter(Boolean).join(", ");
}

/** GitLab numbers merge requests `!12`; the others use `#12`. */
const refOf = (link) => `${link.provider === "gitlab" ? "!" : "#"}${link.number}`;

function badgeFor(link, st, local) {
  const actions = [
    { id: "diff", label: "View changes", primary: true },
    { id: "open", label: `Open in ${PROVIDERS[link.provider].name}` },
    { id: "refresh", label: "Refresh" },
  ];
  const ref = refOf(link);
  const subtitle = `${link.repo} ${ref}`;
  if (!st) return { label: ref, tone: "neutral", icon: "pr-open", title: "Loading pull request…", subtitle, actions };
  if (st.error) {
    return { label: ref, tone: "neutral", icon: "pr-open", title: `Pull request ${ref}`, subtitle, rows: [{ label: "Status", value: st.error, tone: "danger" }], actions };
  }
  const { passed, failed, running } = st.checks;
  const total = passed + failed + running;
  const { ahead, behind } = local ?? { ahead: 0, behind: 0 };
  const pending = syncText({ ahead, behind });
  return {
    label: `${ref} ${st.label}`,
    counts: [
      { icon: "push", value: ahead },
      { icon: "pull", value: behind },
    ].filter((c) => c.value > 0),
    tone: st.tone,
    icon: st.icon,
    title: st.title,
    subtitle,
    rows: [
      { label: "Status", value: st.label, tone: st.tone },
      { label: "Review", value: st.review },
      {
        label: "Checks",
        value: total ? `${passed} passed, ${failed} failed, ${running} running` : "None",
        tone: failed ? "danger" : running ? "warning" : total ? "success" : undefined,
      },
      ...(st.changes ? [{ label: "Changes", value: st.changes }] : []),
      { label: "Branch", value: st.branch },
      ...(pending ? [{ label: "Local", value: pending.replace("push", "push to the PR"), tone: "info" }] : []),
      { label: "Updated", value: ago(st.updated) },
    ],
    actions,
  };
}

const problemKey = (remote) => (remote?.kind === "github" ? "github" : remote?.kind === "gitlab" ? `gitlab:${remote.host}` : remote?.org);

function problemBadge(provider, problem) {
  const { name, tone } = PROVIDERS[provider];
  if (!problem.login) {
    return {
      label: "PRs unavailable",
      tone: "warning",
      icon: "pr-open",
      title: name,
      rows: [{ label: "Error", value: problem.message, tone: "warning" }],
      actions: [{ id: "retry", label: "Try again", primary: true }],
    };
  }
  const run = signingIn.get(provider);
  let label = `Sign in to ${name}`;
  let subtitle = problem.message;
  if (run?.code) [label, subtitle] = [`Code ${run.code}`, `Enter this code on the ${name} page in your browser. It's on your clipboard too.`];
  else if (run) [label, subtitle] = ["Signing in…", "Finish signing in in your browser."];
  return {
    label,
    tone,
    icon: "pr-open",
    loading: Boolean(run),
    title: name,
    subtitle,
    actions: [{ id: "signin", label: "Sign in", primary: true }, { id: "retry", label: "Try again" }],
  };
}

function render() {
  for (const pane of panes) {
    const link = links.get(pane.paneId);
    const remote = remotes.get(pane.cwd);
    const problem = problems.get(problemKey(remote));
    if (problem?.login || (problem && !link)) {
      void wings.setBadge(pane.paneId, problemBadge(remote.kind, problem));
    } else if (link) {
      const loading = inFlight.has(link.url) && (!status.has(link.url) || manual.has(link.url));
      void wings.setBadge(pane.paneId, { ...badgeFor(link, status.get(link.url), sync.get(pane.paneId)), loading });
    } else {
      void wings.setBadge(pane.paneId, null);
    }
  }
}

/**
 * Runs the provider's own login so you never need a terminal. `az` and `glab` open the browser themselves.
 * `gh` doesn't without a terminal: it prints a one-time code and a URL, so the badge shows the code and
 * Wings opens the URL.
 */
async function signIn(remote) {
  const provider = remote?.kind;
  if (!PROVIDERS[provider]) return;
  if (signingIn.has(provider)) return;
  signingIn.set(provider, {});
  render();
  try {
    const out =
      provider === "github"
        ? await wings.exec("gh", ["auth", "login", "--web", "--clipboard", "--hostname", "github.com"], {
            timeoutMs: SIGN_IN_TIMEOUT_MS,
            onOutput: (line) => {
              const code = line.match(/code(?::| \()\s*([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1];
              if (code) {
                signingIn.set(provider, { code });
                render();
              }
              const url = line.match(/https:\/\/github\.com\/\S+/)?.[0];
              if (url) void wings.openUrl(url);
            },
          })
        : provider === "gitlab"
          ? await wings.exec("glab", ["auth", "login", "--web", "--hostname", remote.host], { timeoutMs: SIGN_IN_TIMEOUT_MS })
          : await wings.exec("az", ["login", "--allow-no-subscriptions", "--output", "none"], { timeoutMs: SIGN_IN_TIMEOUT_MS });
    if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "Sign-in failed");
  } finally {
    signingIn.delete(provider);
  }
  return retry();
}

function retry() {
  problems.clear();
  lookups.clear();
  status.clear();
  return refreshLinks();
}

wings.onAction(async ({ paneId, actionId }) => {
  const pane = panes.find((p) => p.paneId === paneId);
  if (actionId === "signin") return signIn(pane && remotes.get(pane.cwd));
  if (actionId === "retry") return retry();
  const link = links.get(paneId);
  if (!link) return;
  if (actionId === "open") return wings.openUrl(link.url);
  if (actionId === "refresh") return refreshStatus(link.url, { byUser: true });
  if (actionId === "diff") {
    const st = status.get(link.url);
    const title = st?.title ?? `Pull request ${refOf(link)}`;
    const subtitle = `${link.repo} ${refOf(link)}`;
    const cached = diffs.get(link.url);
    if (cached && cached.key === st?.key) {
      return void (await wings.openDiff({ title, subtitle, patch: cached.patch, comments: cached.comments }));
    }
    // Not prefetched yet: open the viewer right away in its loading state, then fill it in.
    const { id } = await wings.openDiff({ title, subtitle });
    try {
      const { patch, comments } = await loadDiff(link.url);
      await wings.updateDiff(id, { patch, comments });
    } catch (error) {
      await wings.updateDiff(id, { error: String(error.message ?? error) });
    }
  }
});
