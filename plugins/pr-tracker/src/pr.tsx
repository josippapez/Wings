// The PR tab: one pull request with its status, description and review threads. The diff viewer and Send to agent
// belong to the main script, which asks for them by broadcast; everything shown here loads after the page does.

import { useCallback, useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Badge, Button, Card, Empty, Heading, List, ListItem, Loading, Page, Properties, Row, showTab, Spinner, Stack, Text, Toolbar } from "wings-plugin-ui";
import { azureReply } from "./lib/azure";
import { githubReply } from "./lib/github";
import { gitlabReply } from "./lib/gitlab";
import { commentsOf, linkOf, overviewOf, type Overview } from "./lib/refs";
import { PROVIDERS } from "./lib/state";
import type { Comment, Link, PrArgs } from "./lib/types";
import "./pr.css";

const REFRESH_MS = 120_000;
/** Longer than this and main isn't going to answer, so the button stops waiting. */
const REQUEST_TIMEOUT_MS = 30_000;

const BADGE_TONE = { neutral: undefined, info: "info", success: "success", warning: "warn", danger: "danger", merged: "info" } as const;

const ago = (iso: string) => {
  const minutes = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60_000));
  if (minutes < 60) return `${minutes} min ago`;
  return minutes < 1440 ? `${Math.round(minutes / 60)} h ago` : `${Math.round(minutes / 1440)} d ago`;
};

interface Thread {
  root: Comment;
  replies: Comment[];
}

/** Line comments grouped under their first comment; comments on the whole PR come first. */
function threadsOf(comments: Comment[]): Thread[] {
  const roots = comments.filter((c) => c.replyTo === null);
  return roots
    .map((root) => ({ root, replies: comments.filter((c) => c.replyTo === root.id) }))
    .sort((a, b) => Number(a.root.path !== null) - Number(b.root.path !== null));
}

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

const replyWith = (link: Link, root: Comment, body: string) =>
  ({ github: githubReply, azure: azureReply, gitlab: gitlabReply }[link.provider] as (link: Link, root: Comment, body: string) => Promise<Comment>)(link, root, body);

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

  const run = async (op: "diff" | "agent") => {
    setBusy(op);
    setNotice(null);
    try {
      await askMain(op, args);
    } catch (e) {
      setNotice(String((e as Error).message ?? e));
    } finally {
      setBusy(null);
    }
  };

  const st = overview?.status;
  const threads = comments ? threadsOf(comments) : null;
  const toolbar = (
    <Toolbar
      title={`${args.repo} ${args.provider === "gitlab" ? "!" : "#"}${args.number}`}
      actions={
        <>
          <Button variant="outline" size="sm" onClick={() => void load()}>Refresh</Button>
          <Button variant="outline" size="sm" busy={busy === "agent"} onClick={() => run("agent")}>Send to agent</Button>
          <Button variant="outline" size="sm" onClick={() => wings.openUrl(args.url)}>Open in {PROVIDERS[args.provider].name}</Button>
          <Button size="sm" busy={busy === "diff"} onClick={() => run("diff")}>Open diff</Button>
        </>
      }
    />
  );

  return (
    <Page toolbar={toolbar}>
      <Stack gap="l">
        <Stack gap="s">
          <Row gap="m" wrap>
            <Heading level={2}>{st?.title ?? args.title}</Heading>
            {st ? <Badge tone={BADGE_TONE[st.tone]}>{st.label}</Badge> : <Spinner label="Loading" />}
          </Row>
          {notice ? <Text tone="danger">{notice}</Text> : null}
          {error ? <Text tone="danger">{error}</Text> : null}
        </Stack>

        {overview && st ? (
          <Properties
            items={[
              { label: "Author", value: overview.author },
              { label: "Branch", value: st.branch },
              { label: "Review", value: st.review },
              {
                label: "Checks",
                value: st.checks.passed + st.checks.failed + st.checks.running
                  ? `${st.checks.passed} passed, ${st.checks.failed} failed, ${st.checks.running} running`
                  : "None",
              },
              ...(st.changes ? [{ label: "Changes", value: st.changes }] : []),
              ...(st.merge ? [{ label: "Merge", value: st.merge }] : []),
            ]}
          />
        ) : error ? null : (
          <Loading>Loading pull request</Loading>
        )}

        {st?.failing.length ? (
          <Stack gap="s">
            <Heading level={3}>Failing checks</Heading>
            <List compact>
              {st.failing.map((f) => (
                <ListItem key={String(f.id)}>
                  <Text tone="danger">{f.name ?? "Check"}{f.summary ? `: ${f.summary}` : ""}</Text>
                </ListItem>
              ))}
            </List>
          </Stack>
        ) : null}

        {overview ? (
          <Stack gap="s">
            <Heading level={3}>Description</Heading>
            {overview.body.trim() ? <Text as="p" className="pr-body">{overview.body}</Text> : <Text tone="muted">No description.</Text>}
          </Stack>
        ) : null}

        <Stack gap="s">
          <Heading level={3}>Review threads</Heading>
          {!threads ? (
            error ? null : <Loading>Loading threads</Loading>
          ) : threads.length === 0 ? (
            <Empty title="No comments yet" />
          ) : (
            threads.map((t) => (
              <ThreadCard
                key={t.root.id}
                thread={t}
                onReply={async (body) => {
                  const reply = await replyWith(link!, t.root, body);
                  setComments((all) => [...(all ?? []), reply]);
                }}
                canReply={Boolean(link)}
              />
            ))
          )}
        </Stack>
      </Stack>
    </Page>
  );
}

function ThreadCard({ thread, canReply, onReply }: { thread: Thread; canReply: boolean; onReply: (body: string) => Promise<void> }) {
  const { root, replies } = thread;
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const send = async () => {
    if (!text.trim()) return;
    setSending(true);
    setError(null);
    try {
      await onReply(text.trim());
      setText("");
      setOpen(false);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setSending(false);
    }
  };

  return (
    <Card>
      <Stack gap="s">
        {root.path ? <Text mono size="small" tone="muted" className="pr-thread-path">{root.path}{root.line ? `:${root.line}` : ""}</Text> : null}
        {[root, ...replies].map((c) => (
          <Stack gap="xs" key={c.id}>
            <Row gap="s">
              <Text weight="strong">{c.author}</Text>
              <Text tone="muted" size="tiny">{ago(c.createdAt)}</Text>
            </Row>
            <Text as="p" className="pr-body">{c.body}</Text>
          </Stack>
        ))}
        {root.canReply && canReply ? (
          open ? (
            <Stack gap="s">
              <textarea aria-label="Reply" rows={3} value={text} onChange={(e) => setText(e.target.value)} />
              {error ? <Text tone="danger" size="small">{error}</Text> : null}
              <Row gap="s">
                <Button size="sm" busy={sending} onClick={send}>Reply</Button>
                <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
              </Row>
            </Stack>
          ) : (
            <Row><Button size="sm" variant="ghost" onClick={() => setOpen(true)}>Reply</Button></Row>
          )
        ) : null}
      </Stack>
    </Card>
  );
}

const args = wings.viewArgs<PrArgs>();
showTab(args ? <Pr args={args} /> : <Empty title="No pull request" description="Open one from a project's Pull requests list." />, { fill: true });
