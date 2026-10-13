// The PR tab's page, drawn from what it's given: no loading and no talking to Wings in here, so the same component can be
// rendered with made-up data. pr.tsx owns the loading and the requests.

import { memo, useMemo, useState, type ReactNode } from "react";
import { Avatar, Badge, Button, Chip, Empty, Heading, Markdown, Page, Tabs, Text, Toolbar } from "wings-plugin-ui";
import type { Overview } from "../lib/refs";
import { AZURE_VOTES, type AzureVote } from "../lib/vote";
import { PROVIDERS } from "../lib/state";
import { ago, focusOf, kindOf, oneLine, threadsOf, type Kind, type Thread } from "../lib/threads";
import type { Comment, FailureDetail, Failing, PrArgs, Reviewer, Status } from "../lib/types";
import { ChecksView } from "./checks-view";

const BADGE_TONE = { neutral: undefined, info: "info", success: "success", warning: "warn", danger: "danger", merged: "info" } as const;
const VOTE_WORD: Record<Reviewer["vote"], string> = { approved: "approved", changes: "requested changes", waiting: "is waiting for the author", none: "has not voted" };

export interface PrViewProps {
  args: PrArgs;
  overview: Overview | null;
  comments: Comment[] | null;
  error: string | null;
  notice: string | null;
  busy: string | null;
  canReply: boolean;
  onRefresh: () => void;
  onRun: (op: "diff" | "agent", focus?: WingsDiffFocus) => void;
  onReply: (root: Comment, body: string) => Promise<void>;
  /** Approves the pull request as you. Absent where the provider's approval can't be sent from here. */
  onApprove?: () => Promise<void>;
  /** Azure DevOps votes instead of approving: its menu of votes. */
  onVote?: (vote: AzureVote) => Promise<void>;
  /** Why a failed check failed: its log, fetched when its row is opened. */
  onLoadFailure?: (f: Failing) => Promise<FailureDetail[]>;
  /** Goes back to the project's list, when the PR was picked from it in this tab. */
  onBack?: () => void;
}

const when = (iso: string) => new Date(iso).toLocaleString();

function When({ iso, prefix }: { iso: string; prefix?: string }) {
  const text = ago(iso);
  return text ? (
    <time dateTime={iso} title={when(iso)}>
      {prefix}
      {text}
    </time>
  ) : null;
}

/** The clipboard in a sandboxed frame is the browser's choice, so fall back to the old way if it says no. */
async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const box = document.createElement("textarea");
    box.value = text;
    document.body.append(box);
    box.select();
    document.execCommand("copy");
    box.remove();
  }
}

export function PrView(props: PrViewProps) {
  const { args, overview, comments, error, notice, busy, onRefresh, onRun, onApprove, onVote, onLoadFailure, onBack } = props;
  const st = overview?.status;
  const [tab, setTab] = useState<"overview" | "checks" | "files">("overview");
  const checkTotal = st ? st.checks.passed + st.checks.failed + st.checks.running : 0;
  /** Open comment threads, which a comment requirement counts. Null until the conversation has loaded. */
  const unresolved = useMemo(() => (comments ? comments.filter((c) => c.replyTo === null && !c.system && c.thread?.state === "active").length : null), [comments]);
  const [menu, setMenu] = useState(false);
  /** What the user picked and has yet to confirm. */
  const [pending, setPending] = useState<{ label: string; run: () => Promise<void> } | null>(null);
  const [approving, setApproving] = useState(false);
  const [verdict, setVerdict] = useState<{ ok: boolean; text: string } | null>(null);
  const open = st ? !["Merged", "Closed"].includes(st.label) : false;
  const ref = `${args.repo} ${args.provider === "gitlab" ? "!" : "#"}${args.number}`;

  const send = async () => {
    const choice = pending!;
    setApproving(true);
    setVerdict(null);
    try {
      await choice.run();
      setVerdict({ ok: true, text: `${choice.label}: done.` });
    } catch (e) {
      setVerdict({ ok: false, text: String((e as Error).message ?? e) });
    } finally {
      setApproving(false);
      setPending(null);
    }
  };
  const pick = (choice: { label: string; run: () => Promise<void> }) => {
    setMenu(false);
    setPending(choice);
  };

  const toolbar = (
    <Toolbar
      title={onBack ? undefined : ref}
      actions={
        <>
          <Button variant="outline" size="sm" onClick={onRefresh}>Refresh</Button>
          <Button variant="outline" size="sm" busy={busy === "agent"} onClick={() => onRun("agent")}>Send to agent</Button>
          <Button variant="outline" size="sm" onClick={() => void wings.openUrl(args.url)}>Open in {PROVIDERS[args.provider].name}</Button>
          {onApprove && open ? <Button variant="outline" size="sm" busy={approving} aria-expanded={pending !== null} onClick={() => pick({ label: "Approve", run: onApprove })} data-testid="pr-approve">Approve</Button> : null}
          {onVote && open ? <Button variant="outline" size="sm" busy={approving} aria-expanded={menu} aria-haspopup="true" onClick={() => setMenu((m) => !m)} data-testid="pr-vote">Vote ▾</Button> : null}
          <Button size="sm" busy={busy === "diff"} onClick={() => onRun("diff")}>Open diff</Button>
        </>
      }
    >
      {onBack ? (
        <nav className="pr-crumbs" aria-label="Breadcrumb">
          <Button variant="ghost" size="sm" onClick={onBack} data-testid="pr-back">
            <span aria-hidden="true">←</span> All pull requests
          </Button>
          <span aria-hidden="true" className="pr-crumb-sep">/</span>
          <span className="pr-crumb" title={ref}>{ref}</span>
        </nav>
      ) : null}
    </Toolbar>
  );

  return (
    <Page toolbar={toolbar} className="pr-page" data-testid="pr-page">
      <div className="pr-wrap">
        <header className="pr-header">
          <h1 className="pr-title" title={st?.title ?? args.title} data-testid="pr-title">
            {st?.title ?? args.title}
            <span className="pr-number">{args.provider === "gitlab" ? "!" : "#"}{args.number}</span>
          </h1>
          <div className="pr-meta">
            {st ? <Badge tone={BADGE_TONE[st.tone]} data-testid="pr-state">{st.label}</Badge> : null}
            {st?.branch ? <Branch branch={st.branch} /> : null}
            {overview ? (
              <span className="pr-author">
                <Avatar name={overview.author} size="small" />
                <strong>{overview.author}</strong>
                {overview.createdAt ? <Text tone="muted" as="span"><When iso={overview.createdAt} prefix="opened " /></Text> : null}
              </span>
            ) : null}
            {st?.updated ? <Text tone="muted" size="small" as="span"><When iso={st.updated} prefix="updated " /></Text> : null}
          </div>
          {notice ? <Text tone="danger" role="alert">{notice}</Text> : null}
          {menu && onVote ? (
            <div className="pr-card pr-confirm" role="menu" aria-label="Vote" data-testid="pr-vote-menu">
              {(Object.keys(AZURE_VOTES) as AzureVote[]).map((v) => (
                <Button key={v} size="sm" variant="ghost" role="menuitem" onClick={() => pick({ label: AZURE_VOTES[v], run: () => onVote(v) })}>{AZURE_VOTES[v]}</Button>
              ))}
            </div>
          ) : null}
          {pending ? (
            <div className="pr-card pr-confirm" role="group" aria-label="Confirm" data-testid="pr-confirm">
              <Text>{pending.label} on {ref} as you? Everyone on it sees your vote.</Text>
              <div className="pr-reply-actions">
                <Button size="sm" busy={approving} onClick={() => void send()}>{pending.label}</Button>
                <Button size="sm" variant="ghost" onClick={() => setPending(null)}>Cancel</Button>
              </div>
            </div>
          ) : null}
          {verdict ? <Text tone={verdict.ok ? "success" : "danger"} role={verdict.ok ? "status" : "alert"} data-testid="pr-verdict">{verdict.text}</Text> : null}
        </header>

        {error && !overview ? (
          <div className="pr-card pr-error" role="alert" data-testid="pr-error">
            <Text tone="danger">{error}</Text>
            <Button size="sm" variant="outline" onClick={onRefresh}>Try again</Button>
          </div>
        ) : !overview || !st ? (
          <Skeleton />
        ) : (
          <>
            {error ? <Text tone="danger" role="alert">{error}</Text> : null}
            <div className="pr-summary" data-testid="pr-summary">
              <ReviewChip overview={overview} />
              {checkTotal ? <ChecksChip status={st} onOpen={() => setTab("checks")} /> : <Badge variant="outline">No checks</Badge>}
              {st.label === "Conflicts" ? <Badge tone="danger" variant="outline">Conflicts</Badge> : null}
              {st.merge ? <Badge variant="outline" title={`Merge strategy: ${st.merge}`}>{st.merge}</Badge> : null}
              {st.changes ? <Badge variant="outline">{st.changes}</Badge> : null}
            </div>

            <Tabs
              label="Pull request"
              variant="line"
              value={tab}
              onChange={setTab}
              items={[
                { value: "overview", label: `Overview${comments ? ` ${comments.length}` : ""}` },
                { value: "checks", label: `Checks${checkTotal ? ` ${st.checks.passed}/${checkTotal}` : ""}` },
                { value: "files", label: "Files" },
              ]}
            >
              {tab === "overview" ? <OverviewTab {...props} overview={overview} /> : null}
              {tab === "checks" ? <div className="pr-card"><ChecksView status={st} unresolved={unresolved} onLoadFailure={onLoadFailure} /></div> : null}
              {tab === "files" ? (
                <div className="pr-card pr-files">
                  <Text>{st.changes ?? "The changed files open in Wings' diff viewer, with the review comments on their lines."}</Text>
                  <div><Button size="sm" busy={busy === "diff"} onClick={() => onRun("diff")}>Open diff</Button></div>
                </div>
              ) : null}
            </Tabs>
          </>
        )}
      </div>
    </Page>
  );
}

function Branch({ branch }: { branch: string }) {
  const [copied, setCopied] = useState(false);
  const [source, target] = branch.split(" → ");
  return (
    <span className="pr-branch">
      <code title={`Source branch ${source}`}>{source}</code>
      {target ? (
        <>
          <span aria-hidden="true">→</span>
          <code title={`Target branch ${target}`}>{target}</code>
        </>
      ) : null}
      <button
        type="button"
        className="pr-icon-button"
        aria-label={copied ? "Copied" : `Copy branch name ${source}`}
        title={copied ? "Copied" : "Copy branch name"}
        onClick={() => {
          void copyText(source).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          });
        }}
      >
        {copied ? "✓" : "⧉"}
      </button>
    </span>
  );
}

function ReviewChip({ overview }: { overview: Overview }) {
  const { status, reviewers = [] } = overview;
  const tone = reviewers.some((r) => r.vote === "changes" || r.vote === "waiting") || /changes|rejected|waiting for author/i.test(status.review) ? "danger" : /approved/i.test(status.review) ? "success" : undefined;
  return (
    <span className="pr-chip" data-tone={tone} data-testid="pr-review">
      <span>{status.review}</span>
      {reviewers.length ? (
        <span className="pr-reviewers">
          {reviewers.map((r) => (
            <span key={r.name} className="pr-reviewer" data-vote={r.vote} role="img" aria-label={`${r.name} ${VOTE_WORD[r.vote]}`} title={`${r.name} ${VOTE_WORD[r.vote]}`}>
              <Avatar name={r.name} size="small" />
            </span>
          ))}
        </span>
      ) : null}
    </span>
  );
}

function ChecksChip({ status, onOpen }: { status: Status; onOpen: () => void }) {
  const { passed, failed, running } = status.checks;
  const parts = [failed && `${failed} failed`, running && `${running} running`, passed && `${passed} passed`].filter(Boolean).join(" · ");
  return (
    <Chip className="pr-chip" data-tone={failed ? "danger" : running ? "warn" : "success"} aria-label={`${parts}. Show the checks`} onClick={onOpen} data-testid="pr-checks">
      {parts}
      <span aria-hidden="true">›</span>
    </Chip>
  );
}

function Skeleton() {
  return (
    <div className="pr-skeleton" role="status" aria-busy="true" aria-label="Loading pull request" data-testid="pr-skeleton">
      <div className="pr-skel" style={{ width: "40%", height: 14 }} />
      <div className="pr-skel" style={{ width: "100%", height: 90 }} />
      <div className="pr-skel" style={{ width: "100%", height: 120 }} />
      <div className="pr-skel" style={{ width: "100%", height: 70 }} />
    </div>
  );
}

// ---------- overview: description and conversation ----------

function OverviewTab({ overview, comments, error, canReply, onReply, onRun }: PrViewProps & { overview: Overview }) {
  const threads = useMemo(() => (comments ? threadsOf(comments) : null), [comments]);
  const [showResolved, setShowResolved] = useState(false);
  const resolved = threads?.filter((t) => kindOf(t) === "resolved").length ?? 0;
  const diff = useMemo(() => (focus?: WingsDiffFocus) => onRun("diff", focus), [onRun]);

  return (
    <div className="pr-overview">
      <article className="pr-card" aria-label="Description" data-testid="pr-description">
        <CommentHead author={overview.author} iso={overview.createdAt} suffix="opened this" isAuthor />
        {overview.body.trim() ? <Markdown className="pr-md" source={overview.body} /> : <Text tone="muted">No description provided.</Text>}
      </article>

      <Heading level={3} actions={resolved ? <Chip pressed={showResolved} onClick={() => setShowResolved((s) => !s)}>{showResolved ? "Collapse" : "Expand"} {resolved} resolved</Chip> : undefined}>
        Conversation{threads ? ` · ${threads.length}` : ""}
      </Heading>

      {!threads ? (
        error ? null : <div className="pr-skeleton" role="status" aria-busy="true" aria-label="Loading conversation"><div className="pr-skel" style={{ height: 80 }} /><div className="pr-skel" style={{ height: 80 }} /></div>
      ) : threads.length === 0 ? (
        <Empty title="No comments yet" description="Review threads and comments on the pull request show up here." />
      ) : (
        <ol className="pr-timeline" aria-label="Conversation" data-testid="pr-conversation">
          {threads.map((t) => {
            const kind = kindOf(t);
            return (
              <ThreadCard
                key={`${t.root.id}${kind === "resolved" && showResolved ? "-open" : ""}`}
                thread={t}
                kind={kind}
                defaultOpen={kind === "open" || (kind === "resolved" && showResolved)}
                prAuthor={overview.author}
                canReply={canReply}
                onReply={onReply}
                onDiff={diff}
              />
            );
          })}
        </ol>
      )}
    </div>
  );
}

function CommentHead({ author, iso, suffix, isAuthor, children }: { author: string; iso?: string; suffix?: string; isAuthor?: boolean; children?: ReactNode }) {
  return (
    <div className="pr-comment-head">
      <Avatar name={author} />
      <strong className="pr-comment-author">{author}</strong>
      {isAuthor ? <span className="pr-tag">Author</span> : null}
      <Text tone="muted" size="small" as="span">
        {suffix ? `${suffix} ` : null}
        {iso ? <When iso={iso} /> : null}
      </Text>
      {children ? <span className="pr-comment-head-end">{children}</span> : null}
    </div>
  );
}

interface ThreadProps {
  thread: Thread;
  kind: Kind;
  defaultOpen: boolean;
  prAuthor: string;
  canReply: boolean;
  onReply: (root: Comment, body: string) => Promise<void>;
  onDiff: (focus?: WingsDiffFocus) => void;
}

const ThreadCard = memo(function ThreadCard({ thread, kind, defaultOpen, prAuthor, canReply, onReply, onDiff }: ThreadProps) {
  const { root, replies } = thread;
  const [open, setOpen] = useState(defaultOpen);
  const where = root.path ? `${root.path}${root.line ? `:${root.line}` : ""}` : null;
  const status = root.thread ? <Badge variant="outline" tone={root.thread.state === "active" ? "warn" : "success"} data-testid="pr-thread-status">{root.thread.label}</Badge> : null;

  if (!open) {
    return (
      <li className="pr-thread" data-kind={kind} data-collapsed="">
        <button type="button" className="pr-thread-summary" aria-expanded={false} onClick={() => setOpen(true)}>
          <Avatar name={root.author} size="small" />
          <strong>{root.author}</strong>
          <span className="pr-thread-line">{oneLine(root.body) || "No text"}</span>
          {where ? <code className="pr-path">{where}</code> : null}
          {replies.length ? <Text tone="muted" size="tiny" as="span">{replies.length} {replies.length === 1 ? "reply" : "replies"}</Text> : null}
          {status}
          <span aria-hidden="true" className="pr-chevron">▾</span>
        </button>
      </li>
    );
  }

  return (
    <li className="pr-thread" data-kind={kind}>
      <article className="pr-card" aria-label={`Thread by ${root.author}`}>
        <CommentHead author={root.author} iso={root.createdAt} isAuthor={root.author === prAuthor}>
          {status}
          {kind !== "open" ? <Button size="sm" variant="ghost" aria-expanded onClick={() => setOpen(false)}>Collapse</Button> : null}
        </CommentHead>
        {where ? (
          <div className="pr-where">
            <code className="pr-path">{where}</code>
            <Button size="sm" variant="ghost" aria-label={`View ${where} in the diff`} onClick={() => onDiff(focusOf(root))}>View in diff</Button>
          </div>
        ) : null}
        <Body source={root.body} clamp={kind === "automated"} />
        {replies.length ? (
          <ol className="pr-replies" aria-label="Replies">
            {replies.map((r) => (
              <li key={r.id} className="pr-reply">
                <CommentHead author={r.author} iso={r.createdAt} isAuthor={r.author === prAuthor} />
                <Body source={r.body} clamp={kind === "automated"} />
              </li>
            ))}
          </ol>
        ) : null}
        {root.canReply && canReply ? <ReplyBox onSend={(body) => onReply(root, body)} /> : null}
      </article>
    </li>
  );
});

/** A comment's text; a long bot comment shows its first lines until asked for the rest. */
function Body({ source, clamp }: { source: string; clamp: boolean }) {
  const [more, setMore] = useState(false);
  const long = clamp && (source.length > 320 || source.split("\n").length > 7);
  return (
    <div className="pr-body-box">
      <div className="pr-body-clip" data-clamped={long && !more ? "" : undefined}>
        <Markdown className="pr-md" source={source} />
      </div>
      {long ? <Button size="sm" variant="ghost" aria-expanded={more} onClick={() => setMore((m) => !m)}>{more ? "Show less" : "Show more"}</Button> : null}
    </div>
  );
}

/** Its own state, so typing a reply renders this box and nothing else. */
function ReplyBox({ onSend }: { onSend: (body: string) => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const send = async () => {
    if (!text.trim()) return;
    setSending(true);
    setError(null);
    try {
      await onSend(text.trim());
      setText("");
      setOpen(false);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setSending(false);
    }
  };

  if (!open) return <div><Button size="sm" variant="outline" onClick={() => setOpen(true)}>Reply</Button></div>;
  return (
    <div className="pr-reply-box">
      <textarea aria-label="Reply" rows={3} value={text} autoFocus onChange={(e) => setText(e.target.value)} />
      {error ? <Text tone="danger" size="small" role="alert">{error}</Text> : null}
      <div className="pr-reply-actions">
        <Button size="sm" busy={sending} onClick={send}>Reply</Button>
        <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
      </div>
    </div>
  );
}
