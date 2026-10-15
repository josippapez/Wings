// The PR tab: one pull request with its status, description and review threads. The diff viewer and Send to agent
// belong to the main script, which asks for them by broadcast; everything shown here loads after the page does.

import { useCallback, useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Empty, Loading, Page } from "wings-plugin-ui";
import { approveWith, canApprove, commentsOf, failureOf, linkOf, overviewOf, pushesFor, replyWith, snippetFor, voteWith, type Overview } from "../lib/refs";
import type { Push } from "../lib/pushes";
import type { SnippetLine } from "../lib/snippet";
import type { Comment, Failing, Link, PrArgs } from "../lib/types";
import type { AzureVote } from "../lib/vote";
import "./pr.css";
import { PrView } from "../ui/pr-view";
import { ProjectPulls } from "../ui/pulls-list";

export const page = { place: "tab", fill: true };

const REFRESH_MS = 120_000;
/** Longer than this and main isn't going to answer, so the button stops waiting. */
const REQUEST_TIMEOUT_MS = 30_000;

/** Asks the main script to do something and waits for its answer. Every tab hears every answer, so the id is unique across tabs. */
function askMain(op: "diff" | "agent" | "seen", args: PrArgs, focus?: WingsDiffFocus) {
  const rid = crypto.randomUUID();
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      off();
      reject(new Error("Wings didn't answer. Try again."));
    }, REQUEST_TIMEOUT_MS);
    const off = wings.onBroadcast((message) => {
      const m = message as { type?: string; rid?: string; error?: string } | null;
      if (m?.type !== "pr-response" || m.rid !== rid) return;
      clearTimeout(timer);
      off();
      m.error ? reject(new Error(m.error)) : resolve();
    });
    wings.broadcast({ type: "pr-request", rid, op, args, focus }).catch((e: Error) => {
      clearTimeout(timer);
      off();
      reject(e);
    });
  });
}

function Pr({ args, onBack }: { args: PrArgs; onBack?: () => void }) {
  const [link, setLink] = useState<Link | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [comments, setComments] = useState<Comment[] | null>(null);
  const [pushes, setPushes] = useState<Push[]>([]);
  const snippets = useRef(new Map<number, Promise<SnippetLine[] | null>>());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const resolved = useRef<Promise<Link> | null>(null);

  const load = useCallback(async () => {
    try {
      resolved.current ??= linkOf(args);
      const l = await resolved.current;
      setLink(l);
      // The status, description and threads load side by side; each shows as it arrives.
      const [o, c] = await Promise.all([
        overviewOf(l).then((o) => (setOverview(o), o)),
        commentsOf(l).then((c) => (setComments(c), c)),
        pushesFor(l).then(setPushes),
      ]);
      setError(null);
      return { o, c };
    } catch (e) {
      resolved.current = null;
      setError(String((e as Error).message ?? e));
    }
  }, [args]);

  useEffect(() => {
    void load();
    void askMain("seen", args).catch(() => {});
    return pollWhileFocused(() => void load(), REFRESH_MS);
  }, [args, load]);

  // Another tab's reply, or the diff viewer's, shows here at the next refresh; this one's own is added at once.
  useBroadcast(() => {});

  const run = useCallback(async (op: "diff" | "agent", focus?: WingsDiffFocus) => {
    setBusy(op);
    setNotice(null);
    try {
      await askMain(op, args, focus);
    } catch (e) {
      setNotice(String((e as Error).message ?? e));
    } finally {
      setBusy(null);
    }
  }, [args]);

  const onReply = useCallback(
    async (root: Comment, body: string) => {
      const reply = await replyWith(link!, root, body);
      setComments((all) => [...(all ?? []), reply]);
    },
    [link],
  );

  const onApprove = useCallback(async () => {
    await approveWith(link!, overview!.status.key);
    await load();
  }, [link, load, overview]);

  const onVote = useCallback(async (vote: AzureVote) => {
    await voteWith(link!, vote);
    await load();
  }, [link, load]);

  /** Each code thread's lines are read once, when its card is opened. */
  const onSnippet = useCallback(
    (root: Comment) => {
      const hit = snippets.current.get(root.id);
      if (hit) return hit;
      const lines = snippetFor(link!, root, pushes, overview?.status.headCommit ?? null);
      snippets.current.set(root.id, lines);
      return lines;
    },
    [link, pushes, overview],
  );

  const onLoadFailure = useCallback((f: Failing) => failureOf(link!, f), [link]);

  return (
    <PrView
      args={args}
      overview={overview}
      comments={comments}
      pushes={pushes}
      onSnippet={link?.provider === "azure" ? onSnippet : undefined}
      error={error}
      notice={notice}
      busy={busy}
      canReply={Boolean(link)}
      onRefresh={() => void load()}
      onRun={run}
      onReply={onReply}
      onApprove={link && canApprove(link.provider) ? onApprove : undefined}
      onVote={link?.provider === "azure" ? onVote : undefined}
      onLoadFailure={link ? onLoadFailure : undefined}
      onBack={onBack}
    />
  );
}

/** A tab opened with no pull request (from its title bar button) lists the project's open ones; picking one opens it here, so the tab stays one tab. */
function Home() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  const [picked, setPicked] = useState<PrArgs | null>(null);
  useEffect(() => {
    void wings.project().then(setProject);
  }, []);
  if (picked) {
    return <Pr key={picked.url} args={picked} onBack={() => setPicked(null)} />;
  }
  if (project === undefined) return <Loading>Loading pull requests</Loading>;
  if (!project) return <Empty title="No pull request" description="Open one from a project's Pull requests list." />;
  return (
    <Page className="pr-page">
      <ProjectPulls path={project.path} name={project.name} store="tab" onOpen={setPicked} />
    </Page>
  );
}

export default function PrPage() {
  // viewArgs gives a new copy on every call, and Pr reloads when its args change, so read them once.
  const [args] = useState(() => wings.viewArgs<PrArgs>());
  return args ? <Pr args={args} /> : <Home />;
}
