// The PR tab's page, drawn from what it's given: no loading and no talking to Wings in here, so the same component can be
// rendered with made-up data. pr.tsx owns the loading and the requests.

import { memo, useEffect, useMemo, useState, type ReactNode } from "react";
import { Avatar, Badge, Button, Chip, copyText, Empty, Heading, Markdown, Page, Skeleton, Skeletons, Summary, SummaryChip, Tabs, Text, Time, Timeline, Toolbar } from "wings-plugin-ui";
import { AZURE_VOTES, type AzureVote } from "../lib/vote";
import { PROVIDERS, refOf } from "../lib/providers";
import { focusOf, isAutomated, kindOf, oneLine, threadsOf, type Kind, type Thread } from "../lib/threads";
import { dayTime, timelineOf, type Push } from "../lib/pushes";
import type { Problem } from "../lib/messages";
import { snippetSpan, type SnippetLine } from "../lib/snippet";
import type { Comment, FailureDetail, Failing, Overview, PrArgs, Reviewer, Status } from "../lib/types";
import { ChecksView } from "./checks-view";

const BADGE_TONE = { neutral: undefined, info: "info", success: "success", warning: "warn", danger: "danger", merged: "info" } as const;
const VOTE_WORD: Record<Reviewer["vote"], string> = { approved: "approved", changes: "requested changes", waiting: "is waiting for the author", none: "has not voted" };

export interface PrViewProps {
  args: PrArgs;
  overview: Overview | null;
  comments: Comment[] | null;
  /** Pushes to the branch with their commits, where the provider lists them (Azure DevOps). */
  pushes?: Push[];
  /** The lines around a code thread's comment; absent where they can't be read. */
  onSnippet?: (root: Comment) => Promise<SnippetLine[] | null>;
  error: string | null;
  /** The host needs signing in, a token or a CLI: shown with its button in place of the pull request. */
  problem?: Problem | null;
  /** The problem's button is at work. */
  problemBusy?: boolean;
  onProblem?: () => void;
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
  /** The tab to open on, for a fixture; the page opens on the overview. */
  initialTab?: "overview" | "checks" | "files";
}

const when = (iso: string) => new Date(iso).toLocaleString();

export function PrView(props: PrViewProps) {
  const { args, overview, comments, error, problem, problemBusy, onProblem, notice, busy, onRefresh, onRun, onApprove, onVote, onLoadFailure, onBack } = props;
  const st = overview?.status;
  const [tab, setTab] = useState<"overview" | "checks" | "files">(props.initialTab ?? "overview");
  const checkTotal = st ? st.checks.passed + st.checks.failed + st.checks.running : 0;
  /** Open comment threads, which a comment requirement counts. Null until the conversation has loaded. */
  /** Every comment, a bot's too: the Overview tab's count. Pushes are not comments. */
  const people = comments?.length ?? 0;
  const unresolved = useMemo(() => (comments ? comments.filter((c) => c.replyTo === null && !c.system && c.thread?.state === "active").length : null), [comments]);
  const [menu, setMenu] = useState(false);
  /** What the user picked and has yet to confirm. */
  const [pending, setPending] = useState<{ label: string; run: () => Promise<void> } | null>(null);
  const [approving, setApproving] = useState(false);
  const [verdict, setVerdict] = useState<{ ok: boolean; text: string } | null>(null);
  const open = st ? !["Merged", "Closed"].includes(st.label) : false;
  const ref = `${args.repo} ${refOf(args)}`;

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
            <span className="pr-number">{refOf(args)}</span>
          </h1>
          <div className="pr-meta">
            {st ? <Badge tone={BADGE_TONE[st.tone]} data-testid="pr-state">{st.label}</Badge> : null}
            {st?.branch ? <Branch branch={st.branch} /> : null}
            {overview ? (
              <span className="pr-author">
                <Avatar name={overview.author} size="small" />
                <strong>{overview.author}</strong>
                {overview.createdAt ? <Text tone="muted" as="span"><Time iso={overview.createdAt} prefix="opened " /></Text> : null}
              </span>
            ) : null}
            {st?.updated ? <Text tone="muted" size="small" as="span"><Time iso={st.updated} prefix="updated " /></Text> : null}
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

        {problem && !overview ? (
          <div className="pr-card pr-error" role="alert" data-testid="pr-problem">
            <strong>Couldn't load this pull request</strong>
            <Text tone="muted">{problem.message}</Text>
            <Button size="sm" busy={problemBusy} onClick={onProblem}>{problem.action}</Button>
          </div>
        ) : error && !overview ? (
          <div className="pr-card pr-error" role="alert" data-testid="pr-error">
            <strong>Couldn't load this pull request</strong>
            <Text tone="danger" className="pr-error-text">{error}</Text>
            <Button size="sm" variant="outline" onClick={onRefresh}>Try again</Button>
          </div>
        ) : !overview || !st ? (
          <PrSkeleton />
        ) : (
          <>
            {error ? <Text tone="danger" role="alert">{error}</Text> : null}
            <Summary className="pr-summary" data-testid="pr-summary">
              <ReviewChip overview={overview} />
              {checkTotal ? <ChecksChip status={st} onOpen={() => setTab("checks")} /> : <Badge variant="outline">No checks</Badge>}
              {st.label === "Conflicts" ? <Badge tone="danger" variant="outline">Conflicts</Badge> : null}
              {st.merge ? <Badge variant="outline" title={`Merge strategy: ${st.merge}`}>{st.merge}</Badge> : null}
              {st.changes ? <Badge variant="outline">{st.changes}</Badge> : null}
            </Summary>

            <Tabs
              label="Pull request"
              variant="line"
              value={tab}
              onChange={setTab}
              items={[
                { value: "overview", label: `Overview${people ? ` ${people}` : ""}` },
                { value: "checks", label: `Checks${checkTotal ? ` ${st.checks.passed}/${checkTotal}` : ""}` },
                { value: "files", label: "Files" },
              ]}
            >
              {tab === "overview" ? <OverviewTab {...props} overview={overview} /> : null}
              {tab === "checks" ? <div className="pr-card"><ChecksView status={st} unresolved={unresolved} onLoadFailure={onLoadFailure} /></div> : null}
              {tab === "files" ? (
                <div className="pr-card pr-files">
                  <strong>{st.changes ?? "Changed files"}</strong>
                  <Text tone="muted">The changed files open in Wings' diff viewer, with the review comments on their lines.</Text>
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
    <SummaryChip tone={tone} data-testid="pr-review">
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
    </SummaryChip>
  );
}

function ChecksChip({ status, onOpen }: { status: Status; onOpen: () => void }) {
  const { passed, failed, running } = status.checks;
  const parts = [failed && `${failed} failed`, running && `${running} running`, passed && `${passed} passed`].filter(Boolean).join(" · ");
  return (
    <SummaryChip tone={failed ? "danger" : running ? "warn" : "success"} aria-label={`${parts}. Show the checks`} onClick={onOpen} data-testid="pr-checks">
      {parts}
      <span aria-hidden="true">›</span>
    </SummaryChip>
  );
}

function PrSkeleton() {
  return (
    <Skeletons label="Loading pull request" data-testid="pr-skeleton">
      <Skeleton width="40%" height={14} />
      <Skeleton height={90} />
      <Skeleton height={120} />
      <Skeleton height={70} />
    </Skeletons>
  );
}

// ---------- overview: description and conversation ----------

function OverviewTab({ overview, comments, pushes = NO_PUSHES, error, canReply, onReply, onRun, onSnippet }: PrViewProps & { overview: Overview }) {
  // Pushes and threads share one timeline, the newest first, as Azure DevOps lists them.
  const entries = useMemo(() => (comments ? timelineOf(threadsOf(comments), pushes) : null), [comments, pushes]);
  const [showResolved, setShowResolved] = useState(false);
  const resolved = entries?.filter((e) => e.kind === "thread" && kindOf(e.thread) === "resolved").length ?? 0;
  const threads = entries?.filter((e) => e.kind === "thread").length ?? 0;
  const diff = useMemo(() => (focus?: WingsDiffFocus) => onRun("diff", focus), [onRun]);

  return (
    <div className="pr-overview">
      <article className="pr-card" aria-label="Description" data-testid="pr-description">
        <CommentHead author={overview.author} iso={overview.createdAt} suffix="opened this" isAuthor />
        {overview.body.trim() ? <Markdown className="pr-md" source={overview.body} /> : <Text tone="muted">No description provided.</Text>}
      </article>

      <Heading level={3} actions={resolved ? <Chip pressed={showResolved} onClick={() => setShowResolved((s) => !s)}>{showResolved ? "Collapse" : "Expand"} {resolved} resolved</Chip> : undefined}>
        Conversation{entries ? ` · ${threads}` : ""}
      </Heading>

      {!entries ? (
        error ? null : <Skeletons label="Loading conversation"><Skeleton height={80} /><Skeleton height={80} /></Skeletons>
      ) : entries.length === 0 ? (
        <Empty title="No comments yet" description="Review threads and comments on the pull request show up here." />
      ) : (
        <Timeline label="Conversation" data-testid="pr-conversation">
          {entries.map((e) => {
            if (e.kind === "push") return <PushCard key={`push-${e.push.id}`} push={e.push} />;
            const t = e.thread;
            const kind = kindOf(t);
            return (
              <ThreadCard
                key={`${t.root.id}${kind === "resolved" && showResolved ? "-open" : ""}`}
                thread={t}
                kind={kind}
                defaultOpen={kind !== "resolved" || showResolved}
                prAuthor={overview.author}
                canReply={canReply}
                onReply={onReply}
                onDiff={diff}
                onSnippet={onSnippet}
              />
            );
          })}
        </Timeline>
      )}
    </div>
  );
}

const NO_PUSHES: Push[] = [];

/** How many commits a push lists before "Show all". */
const COMMITS_SHOWN = 5;

/** A push: one quiet line, then its commits in muted text. Comments are what people read, so this stays small. */
function PushCard({ push }: { push: Push }) {
  const [all, setAll] = useState(false);
  const shown = all ? push.commits : push.commits.slice(0, COMMITS_SHOWN);
  const count = `${push.commits.length}${push.more ? "+" : ""}`;
  return (
    <li className="pr-push" data-testid="pr-push">
      <span className="pr-iteration" title={`Update ${push.id}`}>{push.id}</span>
      <div className="pr-push-body">
        <div className="pr-push-head">
          <Avatar name={push.author} size="small" />
          <span><strong>{push.author}</strong> {push.created ? "opened the pull request with" : "pushed"} {count} {push.commits.length === 1 && !push.more ? "commit" : "commits"}</span>
          <Text tone="muted" size="small" as="span"><Time iso={push.at} /></Text>
        </div>
        <ul className="pr-commits" aria-label={`Commits in update ${push.id}`}>
          {shown.map((c) => (
            <li key={c.sha}>
              <span className="pr-commit-msg" title={c.message}>{c.message || "No message"}</span>
              <span className="pr-commit-meta">
                <code>{c.sha.slice(0, 7)}</code>
                <span>{c.author}</span>
                <time dateTime={c.at} title={when(c.at)}>{dayTime(c.at)}</time>
              </span>
            </li>
          ))}
        </ul>
        {push.commits.length > COMMITS_SHOWN ? (
          <Button size="sm" variant="ghost" aria-expanded={all} onClick={() => setAll((a) => !a)}>{all ? "Show fewer commits" : `Show all ${count} commits`}</Button>
        ) : null}
      </div>
    </li>
  );
}

function CommentHead({ author, iso, suffix, isAuthor, bot, children }: { author: string; iso?: string; suffix?: string; isAuthor?: boolean; bot?: boolean; children?: ReactNode }) {
  return (
    <div className="pr-comment-head">
      <Avatar name={author} />
      <strong className="pr-comment-author">{author}</strong>
      {isAuthor ? <span className="pr-tag">Author</span> : null}
      {bot ? <span className="pr-tag">Bot</span> : null}
      <Text tone="muted" size="small" as="span">
        {suffix ? `${suffix} ` : null}
        {iso ? <Time iso={iso} /> : null}
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
  onSnippet?: (root: Comment) => Promise<SnippetLine[] | null>;
}

const ThreadCard = memo(function ThreadCard({ thread, kind, defaultOpen, prAuthor, canReply, onReply, onDiff, onSnippet }: ThreadProps) {
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

  const slash = root.path ? root.path.lastIndexOf("/") : -1;
  const file = root.path ? { name: root.path.slice(slash + 1), dir: root.path } : null;
  return (
    <li className="pr-thread" data-kind={kind}>
      <article className="pr-card" aria-label={`Thread by ${root.author}`}>
        {file ? (
          <>
            <div className="pr-file-head">
              <div className="pr-file-id">
                <strong className="pr-file-name">{file.name}</strong>
                <code className="pr-path" title={where ?? undefined}>{file.dir}{root.line ? <span className="pr-line-no">:{root.line}</span> : null}</code>
              </div>
              <Button size="sm" variant="ghost" aria-label={`View ${where} in the diff`} onClick={() => onDiff(focusOf(root))}>View in diff</Button>
            </div>
            {onSnippet ? <Snippet root={root} onSnippet={onSnippet} /> : null}
          </>
        ) : null}
        <CommentHead author={root.author} iso={root.createdAt} isAuthor={root.author === prAuthor} bot={isAutomated(root)}>
          {status}
          {kind === "resolved" ? <Button size="sm" variant="ghost" aria-expanded onClick={() => setOpen(false)}>Collapse</Button> : null}
        </CommentHead>
        <Body source={root.body} clamp={kind === "automated"} />
        {replies.length ? (
          <ol className="pr-replies" aria-label="Replies">
            {replies.map((r) => (
              <li key={r.id} className="pr-reply">
                <CommentHead author={r.author} iso={r.createdAt} isAuthor={r.author === prAuthor} bot={isAutomated(r)} />
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

/** Blank lines numbered like the snippet that will replace them. A file that ends sooner makes the real one a few lines shorter. */
function placeholderLines(start: number, end?: number): SnippetLine[] {
  const { from, to, last } = snippetSpan(start, end);
  return Array.from({ length: to - from + 1 }, (_, i) => ({ n: from + i, text: "", hit: from + i >= start && from + i <= last }));
}

/** The lines around the commented ones, read when the card shows: mono, line numbers muted, the commented lines marked. */
function Snippet({ root, onSnippet }: { root: Comment; onSnippet: (root: Comment) => Promise<SnippetLine[] | null> }) {
  const [lines, setLines] = useState<SnippetLine[] | null>(null);
  useEffect(() => {
    let live = true;
    void onSnippet(root).then((l) => live && setLines(l));
    return () => {
      live = false;
    };
  }, [root, onSnippet]);
  // While the file is read, empty lines of the count the range gives keep the card's height, so it doesn't jump when the code arrives.
  const shown = lines ?? (root.path && root.range ? placeholderLines(root.range.start, root.range.end) : null);
  if (!shown) return null;
  return (
    <pre className="pr-snippet" aria-label={`Code around line ${root.line}`} aria-busy={lines ? undefined : true} data-testid="pr-snippet" data-loading={lines ? undefined : ""}>
      {shown.map((l) => (
        <code key={l.n} className="pr-snippet-line" data-hit={l.hit ? "" : undefined} data-added={l.added ? "" : undefined}>
          <span className="pr-ln" aria-hidden="true">{l.n}</span>
          <span className="pr-code">{l.text || " "}</span>
        </code>
      ))}
    </pre>
  );
}

/** A comment's text; a long bot comment shows its first lines until asked for the rest. */
function Body({ source, clamp }: { source: string; clamp: boolean }) {
  const [more, setMore] = useState(false);
  const long = clamp && (source.length > 1600 || source.split("\n").length > 24);
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
