// The PR tab: one pull request with its status, description and review threads. The diff viewer and Send to agent
// belong to the main script, which is asked for them; everything shown here loads after the page does.

import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { Page, ProjectGate, useAction, usePoll, useViewArgs } from "wings-plugin-ui";
import { readArgs } from "../lib/args";
import { forgeOf, linkOf } from "../lib/forges/registry";
import { ch } from "../lib/messages";
import type { Push } from "../lib/pushes";
import type { SnippetLine } from "../lib/snippet";
import type { Comment, Failing, Link, Overview, PrArgs } from "../lib/types";
import type { AzureVote } from "../lib/vote";
import "./pr.css";
import { PrView } from "../ui/pr-view";
import { ProjectPulls } from "../ui/pulls-list";

export const page = { place: "tab", fill: true };

const REFRESH_MS = 120_000;

function Pr({ args, onBack }: { args: PrArgs; onBack?: () => void }) {
  const [link, setLink] = useState<Link | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [comments, setComments] = useState<Comment[] | null>(null);
  const [pushes, setPushes] = useState<Push[]>([]);
  const snippets = useRef(new Map<number, Promise<SnippetLine[] | null>>());
  const [error, setError] = useState<string | null>(null);
  /** Which button waits on main: the action's own flag is for all of them. */
  const [busy, setBusy] = useState<"diff" | "agent" | null>(null);
  const action = useAction();
  const resolved = useRef<Promise<Link> | null>(null);

  const load = useCallback(async () => {
    try {
      resolved.current ??= linkOf(args);
      const l = await resolved.current;
      setLink(l);
      // The status, description and threads load side by side; each shows as it arrives.
      const [o, c] = await Promise.all([
        forgeOf(l).overview(l).then((o) => (setOverview(o), o)),
        forgeOf(l).comments(l).then((c) => (setComments(c), c)),
        (forgeOf(l).pushes?.(l) ?? Promise.resolve([])).then(setPushes),
      ]);
      setError(null);
      return { o, c };
    } catch (e) {
      resolved.current = null;
      setError(errorMessage(e));
    }
  }, [args]);

  usePoll(load, REFRESH_MS);
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

  const onReply = useCallback(
    async (root: Comment, body: string) => {
      const reply = await forgeOf(link!).reply(link!, root, body);
      setComments((all) => [...(all ?? []), reply]);
    },
    [link],
  );

  const onApprove = useCallback(async () => {
    await forgeOf(link!).approve!(link!, overview!.status.key);
    await load();
  }, [link, load, overview]);

  const onVote = useCallback(async (vote: AzureVote) => {
    await forgeOf(link!).vote!(link!, vote);
    await load();
  }, [link, load]);

  /** Each code thread's lines are read once, when its card is opened. */
  const onSnippet = useCallback(
    (root: Comment) => {
      const hit = snippets.current.get(root.id);
      if (hit) return hit;
      const lines = forgeOf(link!).snippet!(link!, root, pushes, overview?.status.headCommit ?? null);
      snippets.current.set(root.id, lines);
      return lines;
    },
    [link, pushes, overview],
  );

  const onLoadFailure = useCallback((f: Failing) => forgeOf(link!).failure(link!, f), [link]);

  return (
    <PrView
      args={args}
      overview={overview}
      comments={comments}
      pushes={pushes}
      onSnippet={link && forgeOf(link).snippet ? onSnippet : undefined}
      error={error}
      notice={action.error}
      busy={busy}
      canReply={Boolean(link)}
      onRefresh={() => void load()}
      onRun={run}
      onReply={onReply}
      onApprove={link && forgeOf(link).approve ? onApprove : undefined}
      onVote={link && forgeOf(link).vote ? onVote : undefined}
      onLoadFailure={link ? onLoadFailure : undefined}
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
