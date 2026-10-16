// The PR tab: one pull request with its status, description and review threads. The main script loads them as queries,
// shared with every tab on the PR and with a pane's badge on it, and does what is asked of it: the diff viewer, Send to
// agent, replies, approvals and votes.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "wings-plugin-sdk/react";
import { Page, ProjectGate, useAction, useViewArgs } from "wings-plugin-ui";
import { prKey, readArgs } from "../lib/args";
import { ch } from "../lib/messages";
import type { Push } from "../lib/pushes";
import type { SnippetLine } from "../lib/snippet";
import type { Comment, Failing, PrArgs } from "../lib/types";
import type { AzureVote } from "../lib/vote";
import "./pr.css";
import { PrView } from "../ui/pr-view";
import { ProjectPulls } from "../ui/pulls-list";

export const page = { place: "tab", fill: true };

/** GitLab and Azure DevOps read an overview in two calls in a row, so main gets longer than an ask's 30 s. */
const TIMEOUT_MS = 90_000;
/** A sign-in waits for the browser for up to 5 minutes, and main answers when it is done. */
const ACTION_TIMEOUT_MS = 330_000;
const NO_PUSHES: Push[] = [];

function Pr({ args, onBack }: { args: PrArgs; onBack?: () => void }) {
  const key = useMemo(() => prKey(args), [args]);
  const pr = useQuery(ch, "pr", key, { timeoutMs: TIMEOUT_MS });
  const comments = useQuery(ch, "pr-comments", key, { timeoutMs: TIMEOUT_MS });
  const pushes = useQuery(ch, "pr-pushes", key, { timeoutMs: TIMEOUT_MS });
  const snippets = useRef(new Map<string, Promise<SnippetLine[] | null>>());
  /** Which button waits on main: the action's own flag is for all of them. */
  const [busy, setBusy] = useState<"diff" | "agent" | null>(null);
  const action = useAction();
  const signIn = useAction();
  const overview = pr.data?.overview ?? null;
  const can = pr.data?.can;

  // Opening the tab marks the PR read, and tells main which project it came from.
  useEffect(() => {
    void ch.ask("pr", { op: "seen", args }).catch(() => {});
  }, [args]);

  const { run: act } = action;
  const run = useCallback(
    (op: "diff" | "agent", focus?: WingsDiffFocus) => {
      setBusy(op);
      return act(() => ch.ask("pr", { op, args, focus })).finally(() => setBusy(null));
    },
    [args, act],
  );

  const refresh = () => {
    pr.refresh();
    comments.refresh();
    pushes.refresh();
  };

  // Main puts the reply into the conversation that every tab on the PR shows.
  const onReply = useCallback(async (root: Comment, body: string) => void (await ch.ask("pr-reply", { args, root, body })), [args]);
  const onApprove = useCallback(() => ch.ask("pr", { op: "approve", args, sha: overview!.status.key }), [args, overview]);
  const onVote = useCallback((vote: AzureVote) => ch.ask("pr", { op: "vote", args, vote }), [args]);

  /** Each code thread's lines are read once per view (as commented, or now), when its card shows them. */
  const onSnippet = useCallback(
    (root: Comment, now: boolean) => {
      const key = `${root.id}:${now}`;
      const hit = snippets.current.get(key);
      if (hit) return hit;
      const lines = ch.ask("pr-snippet", { args, root, head: overview?.status.headCommit ?? null, now });
      snippets.current.set(key, lines);
      return lines;
    },
    [args, overview],
  );

  const onLoadFailure = useCallback((failing: Failing) => ch.ask("pr-failure", { args, failing }), [args]);

  return (
    <PrView
      args={args}
      overview={overview}
      comments={comments.data}
      pushes={pushes.data ?? NO_PUSHES}
      onSnippet={can?.snippet ? onSnippet : undefined}
      error={(pr.error ?? comments.error ?? pushes.error)?.message ?? null}
      problem={pr.data?.problem}
      problemBusy={signIn.busy}
      onProblem={() => void signIn.run(() => ch.ask("pulls-action", { path: args.cwd }, { timeoutMs: ACTION_TIMEOUT_MS }))}
      notice={action.error ?? signIn.error}
      busy={busy}
      canReply={Boolean(overview)}
      onRefresh={refresh}
      onRun={run}
      onReply={onReply}
      onApprove={can?.approve ? onApprove : undefined}
      onVote={can?.vote ? onVote : undefined}
      onLoadFailure={overview ? onLoadFailure : undefined}
      onBack={onBack}
    />
  );
}

/** The project's list, and the pull request picked from it, which the tab shows in its place. The pick is in here so the gate above it isn't shown again when the tab goes back. */
function Picker({ project }: { project: WingsProject }) {
  const [picked, setPicked] = useState<PrArgs | null>(null);
  if (picked) return <Pr key={picked.url} args={picked} onBack={() => setPicked(null)} />;
  return (
    <Page className="pr-page">
      <ProjectPulls path={project.path} name={project.name} store="tab" onOpen={setPicked} />
    </Page>
  );
}

export default function PrPage() {
  const args = useViewArgs(readArgs);
  if (args) return <Pr args={args} />;
  // A tab opened with no pull request (from its title bar button) lists the project's open ones; picking one opens it here, so the tab stays one tab.
  return (
    <ProjectGate loading="Loading pull requests" empty={{ title: "No pull request", description: "Open one from a project's Pull requests list." }}>
      {(project) => <Picker project={project} />}
    </ProjectGate>
  );
}
