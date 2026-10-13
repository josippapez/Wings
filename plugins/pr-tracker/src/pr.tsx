// The PR tab: one pull request with its status, description and review threads. The diff viewer and Send to agent
// belong to the main script, which asks for them by broadcast; everything shown here loads after the page does.

import { useCallback, useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Empty, showTab } from "wings-plugin-ui";
import { commentsOf, linkOf, overviewOf, replyWith, type Overview } from "./lib/refs";
import type { Comment, Link, PrArgs } from "./lib/types";
import "./pr.css";
import { PrView } from "./ui/pr-view";

const REFRESH_MS = 120_000;
/** Longer than this and main isn't going to answer, so the button stops waiting. */
const REQUEST_TIMEOUT_MS = 30_000;

/** Asks the main script to do something and waits for its answer. Every tab hears every answer, so the id is unique across tabs. */
function askMain(op: "diff" | "agent" | "seen", args: PrArgs) {
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
    wings.broadcast({ type: "pr-request", rid, op, args }).catch((e: Error) => {
      clearTimeout(timer);
      off();
      reject(e);
    });
  });
}

function Pr({ args }: { args: PrArgs }) {
  const [link, setLink] = useState<Link | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [comments, setComments] = useState<Comment[] | null>(null);
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

  const run = useCallback(async (op: "diff" | "agent") => {
    setBusy(op);
    setNotice(null);
    try {
      await askMain(op, args);
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

  return (
    <PrView
      args={args}
      overview={overview}
      comments={comments}
      error={error}
      notice={notice}
      busy={busy}
      canReply={Boolean(link)}
      onRefresh={() => void load()}
      onRun={run}
      onReply={onReply}
    />
  );
}

const args = wings.viewArgs<PrArgs>();
showTab(args ? <Pr args={args} /> : <Empty title="No pull request" description="Open one from a project's Pull requests list." />, { fill: true });
