// ---------- badge ----------
import { CLIS, failKey, failures, inFlight, links, manual, PROVIDERS, problems, refOf, remotes, signingIn, state, status, sync } from "./state";
import type { Link, Problem, Provider, Remote, Status, StatusEntry } from "./types";

function ago(iso: string) {
  const minutes = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 24 ? `${hours} h ago` : `${Math.round(hours / 24)} d ago`;
}

function syncText({ ahead, behind }: { ahead: number; behind: number }) {
  return [ahead && `${ahead} to push`, behind && `${behind} to pull`].filter(Boolean).join(", ");
}

function badgeFor(link: Link, entry: StatusEntry | undefined, local: { ahead: number; behind: number } | undefined): WingsBadge {
  const actions = [
    { id: "diff", label: "View changes", primary: true },
    { id: "open", label: `Open in ${PROVIDERS[link.provider].name}` },
    { id: "refresh", label: "Refresh" },
  ];
  const ref = refOf(link);
  const subtitle = `${link.repo} ${ref}`;
  if (!entry) return { label: ref, tone: "neutral", icon: "pr-open", title: "Loading pull request…", subtitle, actions };
  if (entry.error) {
    return { label: ref, tone: "neutral", icon: "pr-open", title: `Pull request ${ref}`, subtitle, rows: [{ label: "Status", value: entry.error, tone: "danger" }], actions };
  }
  const st = entry as Status;
  const { passed, failed, running } = st.checks;
  const total = passed + failed + running;
  const { ahead, behind } = local ?? { ahead: 0, behind: 0 };
  const pending = syncText({ ahead, behind });
  const fails = failed ? failures.get(link.url) : null;
  const details = fails?.key === failKey(st) ? (fails.list ?? []) : [];
  return {
    label: `${ref} ${st.label}`,
    counts: (
      [
        { icon: "push", value: ahead },
        { icon: "pull", value: behind },
      ] as const
    ).filter((c) => c.value > 0),
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
      ...details.slice(0, 3).map((f) => ({ label: "Failed", value: `${f.step ?? f.check}: ${f.summary}`, tone: "danger" as const })),
      ...(st.merge ? [{ label: "Merge", value: st.merge }] : []),
      ...(st.changes ? [{ label: "Changes", value: st.changes }] : []),
      { label: "Branch", value: st.branch },
      ...(pending ? [{ label: "Local", value: pending.replace("push", "push to the PR"), tone: "info" as const }] : []),
      { label: "Updated", value: ago(st.updated) },
    ],
    actions,
  };
}

export const problemKey = (remote: Remote | undefined) => (remote?.kind === "github" ? "github" : remote?.kind === "gitlab" ? `gitlab:${remote.host}` : remote?.org);

function problemBadge(provider: Provider, problem: Problem): WingsBadge {
  const { name, tone } = PROVIDERS[provider];
  if (problem.missing) {
    return {
      label: `Install ${CLIS[problem.missing]}`,
      tone,
      icon: "pr-open",
      title: name,
      subtitle: problem.message,
      actions: [{ id: "install", label: "Install", primary: true }, { id: "retry", label: "Try again" }],
    };
  }
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

export function render() {
  for (const pane of state.panes) {
    const link = links.get(pane.paneId);
    const remote = remotes.get(pane.cwd);
    const problem = problems.get(problemKey(remote));
    if (problem?.login || problem?.missing || (problem && !link)) {
      void wings.setBadge(pane.paneId, problemBadge(remote!.kind as Provider, problem));
    } else if (link) {
      const loading = inFlight.has(link.url) && (!status.has(link.url) || manual.has(link.url));
      void wings.setBadge(pane.paneId, { ...badgeFor(link, status.get(link.url), sync.get(pane.paneId)), loading });
    } else {
      void wings.setBadge(pane.paneId, null);
    }
  }
}
