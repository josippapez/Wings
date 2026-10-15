// ---------- badge ----------
import { ago } from "wings-plugin-sdk/format";
import { PROVIDERS, refOf } from "../lib/providers";
import type { Link, Status, StatusEntry } from "../lib/types";
import { codes, problemOf, type Problem } from "./problems";
import { remotes } from "./remote";
import { failKey, failures, inFlight, links, manual, state, status, sync } from "./state";

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

/** What the pane shows while its host has a problem: a button to install the CLI, sign in or add a token, or the error when nothing can be done but try again. */
function problemBadge(problem: Problem): WingsBadge {
  const { name, tone } = PROVIDERS[problem.remote.kind];
  if (problem.kind === "unavailable") {
    return {
      label: "PRs unavailable",
      tone: "warning",
      icon: "pr-open",
      title: name,
      rows: [{ label: "Error", value: problem.message, tone: "warning" }],
      actions: [{ id: "retry", label: "Try again", primary: true }],
    };
  }
  if (problem.kind === "token") {
    return {
      label: `Add ${name} token`,
      tone,
      icon: "pr-open",
      title: name,
      subtitle: problem.message,
      actions: [{ id: "signin", label: "Add token", primary: true }, { id: "retry", label: "Try again" }],
    };
  }
  const { cli, words } = problem.login;
  const key = problem.forge.problemKey(problem.remote);
  const badge = cli.problemBadge(key, { title: name, tone, icon: "pr-open", ...words(problem.remote) })!;
  const code = codes.get(key);
  // While `gh` waits for the code to be entered in the browser, the badge says which.
  return code && badge.loading ? { ...badge, label: `Code ${code}`, subtitle: `Enter this code on the ${name} page in your browser. It's on your clipboard too.` } : badge;
}

export function render() {
  for (const pane of state.panes) {
    const link = links.get(pane.paneId);
    const problem = problemOf(remotes.get(pane.cwd));
    // A problem with the host replaces the PR's badge, except one that only means nothing could be listed.
    if (problem && (problem.kind !== "unavailable" || !link)) {
      void wings.setBadge(pane.paneId, problemBadge(problem));
    } else if (link) {
      const loading = inFlight.has(link.url) && (!status.has(link.url) || manual.has(link.url));
      void wings.setBadge(pane.paneId, { ...badgeFor(link, status.get(link.url), sync.get(pane.paneId)), loading });
    } else {
      void wings.setBadge(pane.paneId, null);
    }
  }
}
