// The PR tab's page, drawn from what it's given: no loading and no talking to Wings in here, so the same component can be
// rendered with made-up data. pr.tsx owns the loading and the requests.

import { memo, useEffect, useMemo, useState, type ReactNode } from "react";
import { Avatar, Badge, Button, Card, Chip, CodeBlock, Confirm, copyText, Empty, Heading, languageOf, ListButton, Markdown, Menu, Notice, Page, Row, Skeleton, Skeletons, Stack, Summary, SummaryChip, Tabs, Text, Textarea, Time, Timeline, Title, Toolbar, type CodeLine } from "wings-plugin-ui";
import { AZURE_VOTES, type AzureVote } from "../lib/vote";
import { PROVIDERS, refOf } from "../lib/providers";
import { focusOf, isAutomated, kindOf, oneLine, threadsOf, type Kind, type Thread } from "../lib/threads";
import { dayTime, timelineOf, type Push } from "../lib/pushes";
import type { Problem } from "../lib/messages";
import { snippetSpan, type SnippetLine } from "../lib/snippet";
import type { Comment, FailureDetail, Failing, Overview, PrArgs, Reviewer, Run, RunRef, RunStep, Status } from "../lib/types";
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
  onSnippet?: (root: Comment, now: boolean) => Promise<SnippetLine[] | null>;
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
  /** Opens one commit's changes in the diff viewer. */
  onCommit?: (commit: { sha: string; message: string }) => void;
  onReply: (root: Comment, body: string) => Promise<void>;
  /** Approves the pull request as you. Absent where the provider's approval can't be sent from here. */
  onApprove?: () => Promise<void>;
  /** Azure DevOps votes instead of approving: its menu of votes. */
  onVote?: (vote: AzureVote) => Promise<void>;
  /** Why a failed check failed: its log, fetched when its row is opened. */
  onLoadFailure?: (f: Failing) => Promise<FailureDetail[]>;
  /** A check's run and a step's log, for the Checks tab's run view. */
  onLoadRun?: (run: RunRef) => Promise<Run>;
  onLoadRunLog?: (run: RunRef, step: RunStep) => Promise<string[]>;
  /** Goes back to the project's list, when the PR was picked from it in this tab. */
  onBack?: () => void;
  /** The tab to open on, for a fixture; the page opens on the overview. */
  initialTab?: "overview" | "checks" | "files";
}

const when = (iso: string) => new Date(iso).toLocaleString();

export function PrView(props: PrViewProps) {
  const { args, overview, comments, error, problem, problemBusy, onProblem, notice, busy, onRefresh, onRun, onApprove, onVote, onLoadFailure, onLoadRun, onLoadRunLog, onBack } = props;
  const st = overview?.status;
  const [tab, setTab] = useState<"overview" | "checks" | "files">(props.initialTab ?? "overview");
  const checkTotal = st ? st.checks.passed + st.checks.failed + st.checks.running : 0;
  /** Open comment threads, which a comment requirement counts. Null until the conversation has loaded. */
  /** Every comment, a bot's too: the Overview tab's count. Pushes are not comments. */
  const people = comments?.length ?? 0;
  const unresolved = useMemo(() => (comments ? comments.filter((c) => c.replyTo === null && !c.system && c.thread?.state === "active").length : null), [comments]);
  /** What the user picked and has yet to confirm. */
  const [pending, setPending] = useState<{ label: string; run: () => Promise<void> } | null>(null);
  const [approving, setApproving] = useState(false);
  const [verdict, setVerdict] = useState<{ ok: boolean; text: string } | null>(null);
  const open = st ? !["Merged", "Closed"].includes(st.label) : false;
  const ref = `${args.repo} ${refOf(args)}`;

  const confirm = pending ? (
    <Confirm
      title={`${pending.label} on ${ref} as you?`}
      cancelLabel="Cancel"
      onCancel={() => setPending(null)}
      actions={[
        {
          id: "send",
          label: pending.label,
          variant: "default",
          onConfirm: async () => {
            setApproving(true);
            setVerdict(null);
            try {
              await pending.run();
              setVerdict({ ok: true, text: `${pending.label}: done.` });
              setPending(null);
            } finally {
              setApproving(false);
            }
          },
        },
      ]}
    >
      <Text>Everyone on it sees your vote.</Text>
    </Confirm>
  ) : null;
  const pick = (choice: { label: string; run: () => Promise<void> }) => setPending(choice);

  const toolbar = (
    <>
      <Toolbar
        title={onBack ? undefined : ref}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={onRefresh}>Refresh</Button>
            <Button variant="outline" size="sm" busy={busy === "agent"} onClick={() => onRun("agent")}>Send to agent</Button>
            <Button variant="outline" size="sm" onClick={() => void wings.openUrl(args.url)}>Open in {PROVIDERS[args.provider].name}</Button>
            {onApprove && open ? <Button variant="outline" size="sm" busy={approving} aria-expanded={pending !== null} onClick={() => pick({ label: "Approve", run: onApprove })} data-testid="pr-approve">Approve</Button> : null}
            {onVote && open ? (
              <Menu
                label="Vote ▾"
                variant="outline"
                size="sm"
                align="end"
                disabled={approving}
                data-testid="pr-vote"
                items={(Object.keys(AZURE_VOTES) as AzureVote[]).map((v) => ({ id: v, label: AZURE_VOTES[v], onSelect: () => pick({ label: AZURE_VOTES[v], run: () => onVote(v) }) }))}
              />
            ) : null}
            <Button size="sm" busy={busy === "diff"} onClick={() => onRun("diff")}>Open diff</Button>
          </>
        }
      >
        {onBack ? (
          <Row gap="s" className="pr-crumbs">
            <Button variant="ghost" size="sm" onClick={onBack} data-testid="pr-back">← All pull requests</Button>
            <Text tone="muted" className="pr-crumb-sep" aria-hidden="true">/</Text>
            <Text tone="muted" truncate className="pr-crumb" title={ref}>{ref}</Text>
          </Row>
        ) : null}
      </Toolbar>
      {confirm}
    </>
  );

  return (
    <Page toolbar={toolbar} className="pr-page" data-testid="pr-page">
      <Stack gap="l" maxWidth={860} className="pr-wrap">
        <Stack as="header" gap="m" className="pr-header">
          <Title className="pr-title" title={st?.title ?? args.title} data-testid="pr-title">
            {st?.title ?? args.title}
            <Text tone="muted" className="pr-number"> {refOf(args)}</Text>
          </Title>
          <Row gap="m" wrap className="pr-meta">
            {st ? <Badge tone={BADGE_TONE[st.tone]} data-testid="pr-state">{st.label}</Badge> : null}
            {st?.branch ? <Branch branch={st.branch} /> : null}
            {overview ? (
              <Row gap="s" className="pr-author">
                <Avatar name={overview.author} size="small" />
                <Text weight="strong">{overview.author}</Text>
                {overview.createdAt ? <Text tone="muted"><Time iso={overview.createdAt} prefix="opened " /></Text> : null}
              </Row>
            ) : null}
            {st?.updated ? <Text tone="muted" size="small"><Time iso={st.updated} prefix="updated " /></Text> : null}
          </Row>
          {notice ? <Notice tone="danger">{notice}</Notice> : null}
          {verdict ? <Notice tone={verdict.ok ? "success" : "danger"} data-testid="pr-verdict">{verdict.text}</Notice> : null}
        </Stack>

        {problem && !overview ? (
          <Card className="pr-card pr-error" data-testid="pr-problem">
            <Stack gap="m">
              <Text weight="strong">Couldn't load this pull request</Text>
              <Text tone="muted">{problem.message}</Text>
              <Row><Button size="sm" busy={problemBusy} onClick={onProblem}>{problem.action}</Button></Row>
            </Stack>
          </Card>
        ) : error && !overview ? (
          <Card className="pr-card pr-error" data-testid="pr-error">
            <Stack gap="m">
              <Text weight="strong">Couldn't load this pull request</Text>
              <Text tone="danger" className="pr-error-text">{error}</Text>
              <Row><Button size="sm" variant="outline" onClick={onRefresh}>Try again</Button></Row>
            </Stack>
          </Card>
        ) : !overview || !st ? (
          <PrSkeleton />
        ) : (
          <>
            {error ? <Notice tone="danger">{error}</Notice> : null}
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
              {tab === "checks" ? <Card className="pr-card"><ChecksView status={st} unresolved={unresolved} onLoadFailure={onLoadFailure} onLoadRun={onLoadRun} onLoadRunLog={onLoadRunLog} /></Card> : null}
              {tab === "files" ? (
                <Card className="pr-card pr-files">
                  <Stack gap="m">
                    <Text weight="strong">{st.changes ?? "Changed files"}</Text>
                    <Text tone="muted">The changed files open in Wings' diff viewer, with the review comments on their lines.</Text>
                    <Row><Button size="sm" busy={busy === "diff"} onClick={() => onRun("diff")}>Open diff</Button></Row>
                  </Stack>
                </Card>
              ) : null}
            </Tabs>
          </>
        )}
      </Stack>
    </Page>
  );
}

function Branch({ branch }: { branch: string }) {
  const [copied, setCopied] = useState(false);
  const [source, target] = branch.split(" → ");
  return (
    <Row gap="s" className="pr-branch">
      <Text mono size="small" truncate title={`Source branch ${source}`}>{source}</Text>
      {target ? (
        <>
          <Text tone="muted" aria-hidden="true">→</Text>
          <Text mono size="small" truncate title={`Target branch ${target}`}>{target}</Text>
        </>
      ) : null}
      <Button
        variant="ghost"
        size="icon-xs"
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
      </Button>
    </Row>
  );
}

function ReviewChip({ overview }: { overview: Overview }) {
  const { status, reviewers = [] } = overview;
  const tone = reviewers.some((r) => r.vote === "changes" || r.vote === "waiting") || /changes|rejected|waiting for author/i.test(status.review) ? "danger" : /approved/i.test(status.review) ? "success" : undefined;
  return (
    <SummaryChip tone={tone} data-testid="pr-review">
      {status.review}
      {reviewers.length ? (
        <Row gap="s" className="pr-reviewers">
          {reviewers.map((r) => (
            <Row key={r.name} className="pr-reviewer" data-vote={r.vote} role="img" aria-label={`${r.name} ${VOTE_WORD[r.vote]}`} title={`${r.name} ${VOTE_WORD[r.vote]}`}>
              <Avatar name={r.name} size="small" />
            </Row>
          ))}
        </Row>
      ) : null}
    </SummaryChip>
  );
}

function ChecksChip({ status, onOpen }: { status: Status; onOpen: () => void }) {
  const { passed, failed, running } = status.checks;
  const parts = [failed && `${failed} failed`, running && `${running} running`, passed && `${passed} passed`].filter(Boolean).join(" · ");
  return (
    <SummaryChip tone={failed ? "danger" : running ? "warn" : "success"} aria-label={`${parts}. Show the checks`} onClick={onOpen} data-testid="pr-checks">
      {`${parts} ›`}
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

/** The native app draws a page of at most 5,000 nodes, so a long conversation shows in steps, as a forge's own page loads more of a long one. The web page shows it all. */
const STEP = WINGS_NATIVE ? 30 : Number.POSITIVE_INFINITY;

function OverviewTab({ overview, comments, pushes = NO_PUSHES, error, canReply, onReply, onRun, onCommit, onSnippet }: PrViewProps & { overview: Overview }) {
  // Pushes and threads share one timeline, the newest first, as Azure DevOps lists them.
  const entries = useMemo(() => (comments ? timelineOf(threadsOf(comments), pushes) : null), [comments, pushes]);
  const [showResolved, setShowResolved] = useState(false);
  const [limit, setLimit] = useState(STEP);
  const resolved = entries?.filter((e) => e.kind === "thread" && kindOf(e.thread) === "resolved").length ?? 0;
  const threads = entries?.filter((e) => e.kind === "thread").length ?? 0;
  const diff = useMemo(() => (focus?: WingsDiffFocus) => onRun("diff", focus), [onRun]);

  return (
    <Stack gap="l" className="pr-overview">
      <Card as="article" className="pr-card" aria-label="Description" data-testid="pr-description">
        <Stack gap="m">
          <CommentHead author={overview.author} iso={overview.createdAt} suffix="opened this" isAuthor />
          {overview.body.trim() ? <Markdown className="pr-md" source={overview.body} /> : <Text tone="muted">No description provided.</Text>}
        </Stack>
      </Card>

      <Heading level={3} actions={resolved ? <Chip pressed={showResolved} onClick={() => setShowResolved((s) => !s)}>{showResolved ? "Collapse" : "Expand"} {resolved} resolved</Chip> : undefined}>
        Conversation{entries ? ` · ${threads}` : ""}
      </Heading>

      {!entries ? (
        error ? null : <Skeletons label="Loading conversation"><Skeleton height={80} /><Skeleton height={80} /></Skeletons>
      ) : entries.length === 0 ? (
        <Empty title="No comments yet" description="Review threads and comments on the pull request show up here." />
      ) : (
        <Timeline label="Conversation" data-testid="pr-conversation">
          {entries.slice(0, limit).map((e) => {
            if (e.kind === "push") return <PushCard key={`push-${e.push.id}`} push={e.push} onCommit={onCommit} />;
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
                latest={pushes.length ? Math.max(...pushes.map((p) => p.id)) : undefined}
              />
            );
          })}
        </Timeline>
      )}
      {entries && entries.length > limit ? (
        <Row><Button variant="outline" onClick={() => setLimit((l) => l + STEP)}>Show {Math.min(STEP, entries.length - limit)} more of {entries.length - limit} not shown</Button></Row>
      ) : null}
    </Stack>
  );
}

const NO_PUSHES: Push[] = [];

/** How many commits a push lists before "Show all". */
const COMMITS_SHOWN = 5;

/** A push: one quiet line, then its commits in muted text. Comments are what people read, so this stays small. */
function PushCard({ push, onCommit }: { push: Push; onCommit?: (commit: { sha: string; message: string }) => void }) {
  const [all, setAll] = useState(false);
  const shown = all ? push.commits : push.commits.slice(0, COMMITS_SHOWN);
  const count = `${push.commits.length}${push.more ? "+" : ""}`;
  return (
    <Row as="li" gap="m" align="start" className="pr-push" data-testid="pr-push">
      <Text mono size="tiny" tone="muted" className="pr-iteration" title={`Update ${push.id}`}>{push.id}</Text>
      <Stack gap="s" grow className="pr-push-body">
        <Row gap="s" wrap className="pr-push-head">
          <Avatar name={push.author} size="small" />
          <Text><Text weight="strong">{push.author}</Text>{` ${push.created ? "opened the pull request with" : "pushed"} ${count} ${push.commits.length === 1 && !push.more ? "commit" : "commits"}`}</Text>
          <Text tone="muted" size="small"><Time iso={push.at} /></Text>
        </Row>
        <Stack as="ul" gap="xs" className="pr-commits" aria-label={`Commits in update ${push.id}`}>
          {shown.map((c) => (
            <Stack as="li" key={c.sha}>
              {onCommit ? (
                <ListButton className="pr-commit" aria-label={`See the changes in ${c.sha.slice(0, 7)}: ${c.message || "No message"}`} onClick={() => onCommit({ sha: c.sha, message: c.message })}>
                  <CommitLine c={c} />
                </ListButton>
              ) : (
                <CommitLine c={c} />
              )}
            </Stack>
          ))}
        </Stack>
        {push.commits.length > COMMITS_SHOWN ? (
          <Row><Button size="sm" variant="ghost" aria-expanded={all} onClick={() => setAll((a) => !a)}>{all ? "Show fewer commits" : `Show all ${count} commits`}</Button></Row>
        ) : null}
      </Stack>
    </Row>
  );
}

function CommitLine({ c }: { c: Push["commits"][number] }) {
  return (
    <>
      <Text truncate grow className="pr-commit-msg" title={c.message}>{c.message || "No message"}</Text>
      <Row gap="s" className="pr-commit-meta">
        <Text mono size="tiny">{c.sha.slice(0, 7)}</Text>
        <Text size="tiny" tone="muted">{c.author}</Text>
        <Text size="tiny" tone="muted" title={when(c.at)}>{dayTime(c.at)}</Text>
      </Row>
    </>
  );
}

function CommentHead({ author, iso, suffix, isAuthor, bot, children }: { author: string; iso?: string; suffix?: string; isAuthor?: boolean; bot?: boolean; children?: ReactNode }) {
  return (
    <Row gap="s" wrap className="pr-comment-head">
      <Avatar name={author} />
      <Text weight="strong" className="pr-comment-author">{author}</Text>
      {isAuthor ? <Badge size="sm" variant="outline" className="pr-tag">Author</Badge> : null}
      {bot ? <Badge size="sm" variant="outline" className="pr-tag">Bot</Badge> : null}
      <Text tone="muted" size="small">
        {suffix ? `${suffix} ` : null}
        {iso ? <Time iso={iso} /> : null}
      </Text>
      {children ? <Row gap="s" className="pr-comment-head-end">{children}</Row> : null}
    </Row>
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
  onSnippet?: (root: Comment, now: boolean) => Promise<SnippetLine[] | null>;
  /** The latest push's number, which a thread written on an older one can switch its code to. */
  latest?: number;
}

const ThreadCard = memo(function ThreadCard({ thread, kind, defaultOpen, prAuthor, canReply, onReply, onDiff, onSnippet, latest }: ThreadProps) {
  const { root, replies } = thread;
  const [open, setOpen] = useState(defaultOpen);
  const where = root.path ? `${root.path}${root.line ? `:${root.line}` : ""}` : null;
  const status = root.thread ? <Badge size="sm" variant="outline" tone={root.thread.state === "active" ? "warn" : "success"} data-testid="pr-thread-status">{root.thread.label}</Badge> : null;

  if (!open) {
    return (
      <Stack as="li" className="pr-thread" data-kind={kind} data-collapsed="">
        <ListButton className="pr-thread-summary" aria-expanded={false} onClick={() => setOpen(true)}>
          <Avatar name={root.author} size="small" />
          <Text weight="strong">{root.author}</Text>
          <Text truncate grow className="pr-thread-line">{oneLine(root.body) || "No text"}</Text>
          {where ? <Text mono size="tiny" truncate className="pr-path">{where}</Text> : null}
          {replies.length ? <Text tone="muted" size="tiny">{replies.length} {replies.length === 1 ? "reply" : "replies"}</Text> : null}
          {status}
          {WINGS_NATIVE ? null : <Text aria-hidden="true" className="pr-chevron">▾</Text>}
        </ListButton>
      </Stack>
    );
  }

  const slash = root.path ? root.path.lastIndexOf("/") : -1;
  const file = root.path ? { name: root.path.slice(slash + 1), dir: root.path } : null;
  return (
    <Stack as="li" className="pr-thread" data-kind={kind}>
      <Card as="article" className="pr-card" aria-label={`Thread by ${root.author}`}>
        <Stack gap="m">
          {file ? (
            <>
              <Row gap="m" justify="between" className="pr-file-head">
                <Stack gap="xs" grow className="pr-file-id">
                  <Text weight="strong" truncate className="pr-file-name">{file.name}</Text>
                  <Text mono size="small" tone="muted" truncate className="pr-path" title={where ?? undefined}>{file.dir}{root.line ? `:${root.line}` : ""}</Text>
                </Stack>
                <Button size="sm" variant="ghost" aria-label={`View ${where} in the diff`} onClick={() => onDiff(focusOf(root))}>View in diff</Button>
              </Row>
              <Snippet root={root} latest={latest} onSnippet={onSnippet} />
            </>
          ) : null}
          <CommentHead author={root.author} iso={root.createdAt} isAuthor={root.author === prAuthor} bot={isAutomated(root)}>
            {status}
            {kind === "resolved" ? <Button size="sm" variant="ghost" aria-expanded onClick={() => setOpen(false)}>Collapse</Button> : null}
          </CommentHead>
          <Body source={root.body} clamp={kind === "automated"} />
          {replies.length ? (
            <Stack as="ol" gap="m" className="pr-replies" aria-label="Replies">
              {replies.map((r) => (
                <Stack as="li" key={r.id} gap="s" className="pr-reply">
                  <CommentHead author={r.author} iso={r.createdAt} isAuthor={r.author === prAuthor} bot={isAutomated(r)} />
                  <Body source={r.body} clamp={kind === "automated"} />
                </Stack>
              ))}
            </Stack>
          ) : null}
          {root.canReply && canReply ? <ReplyBox onSend={(body) => onReply(root, body)} /> : null}
        </Stack>
      </Card>
    </Stack>
  );
});

/** Blank lines numbered like the snippet that will replace them. A file that ends sooner makes the real one a few lines shorter. A thread read from a diff ends at the commented line, so it has no lines below. */
function placeholderLines(start: number, end: number | undefined, below: boolean): SnippetLine[] {
  const { from, to, last } = snippetSpan(start, end);
  return Array.from({ length: (below ? to : last) - from + 1 }, (_, i) => ({ n: from + i, text: "", hit: from + i >= start && from + i <= last }));
}

const codeLine = (l: SnippetLine): CodeLine => ({ text: l.text, n: l.n, kind: l.added ? "added" : l.removed ? "removed" : undefined, highlighted: l.hit });

/**
 * The code a thread is on: the lines the comment carries (GitHub's diff hunk) or the ones main reads when the card
 * shows, with the commented lines marked. A thread written on an older push shows the code as it was then, and can
 * switch to where those lines sit now.
 */
function Snippet({ root, latest, onSnippet }: { root: Comment; latest?: number; onSnippet?: (root: Comment, now: boolean) => Promise<SnippetLine[] | null> }) {
  const [now, setNow] = useState(false);
  const [lines, setLines] = useState<SnippetLine[] | null>(root.snippet ?? null);
  const [gone, setGone] = useState(false);
  useEffect(() => {
    if (root.snippet || !onSnippet) return;
    let live = true;
    setLines(null);
    setGone(false);
    void onSnippet(root, now).then((l) => {
      if (!live) return;
      setLines(l);
      setGone(now && !l);
    });
    return () => {
      live = false;
    };
  }, [root, now, onSnippet]);
  const iteration = root.range?.iteration;
  const older = iteration !== undefined && latest !== undefined && iteration < latest;
  // While the file is read, empty lines of the count the range gives keep the card's height, so it doesn't jump when the code arrives.
  const loading = !lines && Boolean(onSnippet) && Boolean(root.path && root.range) && !gone;
  const shown = lines ?? (loading ? placeholderLines(root.range!.start, root.range!.end, iteration !== undefined) : null);
  const bar = older ? (
    <Row gap="s" justify="between" className="pr-snippet-bar" data-testid="pr-snippet-bar">
      <Text tone="muted" size="tiny">{now ? `Latest code, push ${latest}` : `As of push ${iteration}`}</Text>
      <Button size="sm" variant="ghost" aria-pressed={now} onClick={() => setNow((n) => !n)}>{now ? `Show push ${iteration}` : "Show latest"}</Button>
    </Row>
  ) : null;
  if (gone) {
    return (
      <>
        {bar}
        <Text tone="muted" size="small" className="pr-snippet-gone">These lines are no longer in the latest code.</Text>
      </>
    );
  }
  if (!shown) return null;
  return (
    <>
      {bar}
      <CodeBlock className="pr-snippet" lang={languageOf(root.path)} label={`Code around line ${root.line ?? root.range?.end ?? ""}`} lines={shown.map(codeLine)} busy={!lines} data-testid="pr-snippet" />
    </>
  );
}

/** A comment's text; a long bot comment shows its first lines until asked for the rest. */
function Body({ source, clamp }: { source: string; clamp: boolean }) {
  const [more, setMore] = useState(false);
  const long = clamp && (source.length > 1600 || source.split("\n").length > 24);
  return (
    <Stack gap="s" className="pr-body-box">
      <Markdown className="pr-md" source={long && !more ? source.split("\n").slice(0, 24).join("\n").slice(0, 1600) : source} />
      {long ? <Row><Button size="sm" variant="ghost" aria-expanded={more} onClick={() => setMore((m) => !m)}>{more ? "Show less" : "Show more"}</Button></Row> : null}
    </Stack>
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

  if (!open) return <Row><Button size="sm" variant="outline" onClick={() => setOpen(true)}>Reply</Button></Row>;
  return (
    <Stack gap="s" className="pr-reply-box">
      <Textarea label="Reply" rows={3} value={text} autoFocus onChange={(e) => setText(e.target.value)} />
      {error ? <Notice tone="danger">{error}</Notice> : null}
      <Row gap="s" className="pr-reply-actions">
        <Button size="sm" busy={sending} onClick={send}>Reply</Button>
        <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
      </Row>
    </Stack>
  );
}
