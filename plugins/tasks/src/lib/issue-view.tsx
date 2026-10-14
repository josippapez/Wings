// The Issue tab without its data: one issue with its spec, status, links, sessions and activity. The tab page feeds it
// from storage, a fixture from made-up data.

import { useEffect, useRef, useState } from "react";
import { Badge, Button, Card, Combobox, Empty, Heading, List, ListButton, ListItem, Loading, Markdown, Menu, Page, Row, Select, Stack, Text, TextInput, Time, Timeline, TimelineCard, TimelineNote, Title, Toolbar } from "wings-plugin-ui";
import { openProps, StatusDot, TypeIcon } from "./parts";
import { actorName, blocksOf, canParent, childrenOf, DESCRIPTION_TEMPLATE, isClaude, LIMITS, parentChoices, progress, visibleDescription, lastSession, STATUS_LABEL, STATUSES, TYPE_LABEL, TYPES, type Issue, type LogEntry, type Op, type Status } from "./issues";

export interface IssueViewProps {
  project: WingsProject;
  /** `undefined` while it loads, `null` when the project has no issue with that key. */
  issue: Issue | null | undefined;
  /** Every issue of the project, for the parent, related and blocking pickers and the child list. */
  issues: Issue[];
  /** The key the tab was opened for, for the not-found page. */
  issueKey: string;
  error: string | null;
  send: (op: Op) => Promise<void>;
  onBack: () => void;
  onResume: () => void;
  onStart: () => void;
  onOpenUrl: (url: string) => void;
  /** Open another issue: `newTab` for a middle-click or Cmd/Ctrl+click. */
  onOpenIssue: (key: string, newTab: boolean) => void;
}

const STATUS_OPTIONS = STATUSES.map((value) => ({ value, label: STATUS_LABEL[value] }));
const TYPE_OPTIONS = TYPES.map((value) => ({ value, label: TYPE_LABEL[value] }));
const iso = (ms: number) => new Date(ms).toISOString();

type Send = (op: Op) => Promise<void>;

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

/** A one-line input for editing in place: Enter or leaving it saves, Escape cancels. Empty saves count as a cancel when `required`. */
function InlineInput({ label, value, max, placeholder, required, className, onSave, onDone }: { label: string; value: string; max: number; placeholder?: string; required?: boolean; className?: string; onSave: (value: string) => void; onDone: () => void }) {
  const [draft, setDraft] = useState(value);
  const done = useRef(false);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const finish = (save: boolean) => {
    if (done.current) return;
    done.current = true;
    const next = draft.trim();
    if (save && next !== value && !(required && !next)) onSave(next);
    onDone();
  };
  return (
    <TextInput
      ref={ref}
      label={label}
      className={className}
      value={draft}
      maxLength={max}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={() => finish(true)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(true);
        if (e.key === "Escape") finish(false);
      }}
    />
  );
}

/** The title, large, with the key beside it. Click it, or press Enter or F2 on it, to edit. */
function IssueTitle({ issue, send }: { issue: Issue; send: Send }) {
  const [editing, setEditing] = useState(false);
  return (
    <div className="tk-title-row">
      <TypeIcon type={issue.type} label />
      {editing ? (
        <InlineInput label="Title" className="tk-title-input" value={issue.title} max={LIMITS.title} required onSave={(title) => void send({ kind: "update", key: issue.key, title })} onDone={() => setEditing(false)} />
      ) : (
        <Title as="h1" className="tk-title">
          <Button variant="ghost" className="tk-title-button" title="Edit the title" onClick={() => setEditing(true)} onKeyDown={(e) => { if (e.key === "F2") setEditing(true); }}>
            {issue.title}
          </Button>
        </Title>
      )}
      <Text as="span" mono tone="muted" size="small" className="tk-title-key">{issue.key}</Text>
    </div>
  );
}

function Description({ issue, send }: { issue: Issue; send: Send }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const edit = () => {
    setDraft(issue.description || DESCRIPTION_TEMPLATE);
    setEditing(true);
  };
  const save = async () => {
    setBusy(true);
    try {
      await send({ kind: "update", key: issue.key, description: draft });
      setEditing(false);
    } finally {
      setBusy(false);
    }
  };
  if (editing) {
    return (
      <Stack gap="s">
        <textarea autoFocus aria-label="Description" rows={12} maxLength={LIMITS.description} value={draft} placeholder="Goal, scope and acceptance criteria. Markdown works." onChange={(e) => setDraft(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Escape") setEditing(false); }} className="tk-textarea" />
        <Row gap="s" justify="end">
          <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>Cancel</Button>
          <Button size="sm" busy={busy} onClick={() => void save()}>Save</Button>
        </Row>
      </Stack>
    );
  }
  const shown = visibleDescription(issue.description);
  if (!shown) {
    return <Button variant="ghost" className="tk-empty-description" onClick={edit}>Add a description…</Button>;
  }
  return (
    <div className="tk-description">
      <Markdown source={shown} className="tk-description-text" />
      <Button size="xs" variant="ghost" onClick={edit}>Edit</Button>
    </div>
  );
}

/** One property of the column: a small muted label over its value. */
function Prop({ label, wide, empty, actions, children }: { label: string; wide?: boolean; empty?: boolean; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="tk-prop" data-wide={wide ? "" : undefined} data-empty={empty ? "" : undefined}>
      <Row gap="s" justify="between" className="tk-prop-label">
        <Text as="span" tone="muted" size="tiny" weight="medium">{label}</Text>
        {actions}
      </Row>
      {children}
    </div>
  );
}

function Branch({ issue, project, send }: { issue: Issue; project: WingsProject; send: Send }) {
  const branch = issue.links?.branch ?? "";
  const [editing, setEditing] = useState(false);
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void copyText(branch).then(() => setCopied(true));
    setTimeout(() => setCopied(false), 1500);
  };
  const set = (value: string) => void send({ kind: "update", key: issue.key, branch: value });
  return (
    <Prop
      label="Branch"
      actions={
        editing ? null : (
          <Row gap="xs">
            {branch ? <Button size="xs" variant="ghost" aria-label="Copy the branch name" onClick={copy}>{copied ? "Copied" : "Copy"}</Button> : null}
            <Button size="xs" variant="ghost" aria-label="Edit the branch" onClick={() => setEditing(true)}>Edit</Button>
          </Row>
        )
      }
    >
      {editing ? (
        <InlineInput label="Branch" className="tk-prop-input" value={branch} max={LIMITS.branch} placeholder="feature/name" onSave={set} onDone={() => setEditing(false)} />
      ) : branch ? (
        <Text as="span" mono size="small" className="tk-prop-mono" title={branch}>{branch}</Text>
      ) : (
        <Text as="span" tone="muted" size="small">None</Text>
      )}
      {!editing && project.branch && project.branch !== branch ? (
        <Button size="xs" variant="ghost" className="tk-prop-use" onClick={() => set(project.branch ?? "")}>Use {project.branch}</Button>
      ) : null}
    </Prop>
  );
}

const prLabel = (url: string) => {
  const m = /\/(?:pull|pullrequest|pullrequests|merge_requests)\/(\d+)/.exec(url);
  if (m) return `PR #${m[1]}`;
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

function PullRequest({ issue, send, onOpenUrl }: { issue: Issue; send: Send; onOpenUrl: (url: string) => void }) {
  const pr = issue.links?.pr ?? "";
  const [editing, setEditing] = useState(false);
  return (
    <Prop label="Pull request" actions={editing ? null : <Button size="xs" variant="ghost" aria-label="Edit the pull request link" onClick={() => setEditing(true)}>Edit</Button>}>
      {editing ? (
        <InlineInput label="Pull request URL" className="tk-prop-input" value={pr} max={LIMITS.pr} placeholder="https://" onSave={(value) => void send({ kind: "update", key: issue.key, pr: value })} onDone={() => setEditing(false)} />
      ) : pr ? (
        <Button variant="outline" size="xs" className="tk-pr-chip" title={pr} onClick={() => onOpenUrl(pr)}>{prLabel(pr)} <span aria-hidden="true">↗</span></Button>
      ) : (
        <Text as="span" tone="muted" size="small">None</Text>
      )}
    </Prop>
  );
}

/** Another issue as a chip, "KEY title", that opens it. With `onRemove`, an × beside it takes the link away. */
function IssueChip({ issue, fallback, onOpen, onRemove }: { issue?: Issue; fallback: string; onOpen: (key: string, newTab: boolean) => void; onRemove?: () => void }) {
  return (
    <div className="tk-link">
      {issue ? (
        <Button variant="outline" size="xs" className="tk-link-chip" title={`${issue.key} ${issue.title}`} {...openProps((newTab) => onOpen(issue.key, newTab))}>
          <StatusDot status={issue.status} />
          <span className="tk-key">{issue.key}</span>
          <span className="tk-link-title">{issue.title}</span>
        </Button>
      ) : (
        <Text as="span" mono tone="muted" size="small">{fallback}</Text>
      )}
      {onRemove ? <Button size="icon-xs" variant="ghost" aria-label={`Remove ${fallback}`} onClick={onRemove}><span aria-hidden="true">×</span></Button> : null}
    </div>
  );
}

/** A field that finds an issue by key or title and hands back its key. */
function IssuePicker({ label, placeholder, options, onPick }: { label: string; placeholder: string; options: Issue[]; onPick: (key: string) => void }) {
  const [query, setQuery] = useState("");
  return (
    <Combobox
      freeText
      label={label}
      placeholder={placeholder}
      className="tk-prop-input"
      value={query}
      onInput={setQuery}
      options={options.map((o) => ({ value: o.key, label: o.key, detail: o.title }))}
      onPick={(o) => {
        setQuery("");
        onPick(o.value);
      }}
      empty="No issue matches"
    />
  );
}

function Links({ issue, issues, send, onOpenIssue }: { issue: Issue; issues: Issue[]; send: Send; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const find = (key: string) => issues.find((i) => i.key === key);
  const others = issues.filter((i) => i.key !== issue.key);
  const blocks = blocksOf(issues, issue.key);
  const chips = (keys: string[] | undefined, remove: (key: string) => void) => (keys ?? []).map((k) => <IssueChip key={k} issue={find(k)} fallback={k} onOpen={onOpenIssue} onRemove={() => remove(k)} />);
  return (
    <>
      <Prop label="Parent" wide>
        {issue.parent ? <IssueChip issue={find(issue.parent)} fallback={issue.parent} onOpen={onOpenIssue} onRemove={() => void send({ kind: "update", key: issue.key, parent: "" })} /> : null}
        <IssuePicker label="Parent" placeholder={issue.parent ? "Change parent" : "Choose an epic or feature"} options={parentChoices(issues, issue).filter((p) => p.key !== issue.parent)} onPick={(parent) => void send({ kind: "update", key: issue.key, parent })} />
      </Prop>
      <Prop label="Related" wide empty={!issue.related?.length}>
        {chips(issue.related, (k) => void send({ kind: "update", key: issue.key, relatedRemove: [k] }))}
        <IssuePicker label="Related" placeholder="Add a related issue" options={others.filter((o) => !issue.related?.includes(o.key))} onPick={(k) => void send({ kind: "update", key: issue.key, relatedAdd: [k] })} />
      </Prop>
      <Prop label="Blocked by" wide empty={!issue.blockedBy?.length}>
        {chips(issue.blockedBy, (k) => void send({ kind: "update", key: issue.key, blockedByRemove: [k] }))}
        <IssuePicker label="Blocked by" placeholder="Add a blocking issue" options={others.filter((o) => !issue.blockedBy?.includes(o.key))} onPick={(k) => void send({ kind: "update", key: issue.key, blockedByAdd: [k] })} />
      </Prop>
      {blocks.length ? (
        <Prop label="Blocks" wide>
          {blocks.map((b) => <IssueChip key={b.key} issue={b} fallback={b.key} onOpen={onOpenIssue} />)}
        </Prop>
      ) : null}
    </>
  );
}

/** An epic's or feature's children with how many are done. */
function Children({ issue, issues, onOpenIssue }: { issue: Issue; issues: Issue[]; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const children = childrenOf(issues, issue.key).sort((a, b) => keyOrder(a) - keyOrder(b));
  const { done, total } = progress(children);
  return (
    <Stack gap="s" as="section" aria-label="Child issues">
      <Heading actions={<Text tone="muted" size="small">{total ? `${done} of ${total} done` : null}</Text>}>Child issues</Heading>
      {total ? <div className="tk-progress" role="progressbar" aria-label="Child issues done" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done}><span style={{ width: `${(done / total) * 100}%` }} /></div> : null}
      {children.length === 0 ? (
        <Text tone="muted" size="small">None yet. Open an issue and choose this one as its parent.</Text>
      ) : (
        <List compact aria-label={`Child issues of ${issue.key}`}>
          {children.map((c) => (
            <ListItem key={c.key}>
              <ListButton aria-label={`Open ${c.key}: ${c.title}`} {...openProps((newTab) => onOpenIssue(c.key, newTab))}>
                <StatusDot status={c.status} />
                <TypeIcon type={c.type} />
                <span className="tk-key">{c.key}</span>
                <Text as="span" grow truncate>{c.title}</Text>
                <Text as="span" tone="muted" size="tiny">{STATUS_LABEL[c.status]}</Text>
              </ListButton>
            </ListItem>
          ))}
        </List>
      )}
    </Stack>
  );
}
const keyOrder = (i: Issue) => Number(i.key.slice(i.key.lastIndexOf("-") + 1));

function Properties({ issue, issues, project, send, onOpenUrl, onOpenIssue }: { issue: Issue; issues: Issue[]; project: WingsProject; send: Send; onOpenUrl: (url: string) => void; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const last = lastSession(issue);
  const commits = issue.links?.commits ?? [];
  return (
    <aside className="tk-props" aria-label="Properties">
      <Prop label="Status">
        <Row gap="s" className="tk-status">
          <StatusDot status={issue.status} />
          <Select label="Status" className="tk-status-select" options={STATUS_OPTIONS} value={issue.status} onChange={(status: Status) => void send({ kind: "update", key: issue.key, status })} />
        </Row>
      </Prop>
      <Prop label="Type">
        <Row gap="s" className="tk-status">
          <TypeIcon type={issue.type} />
          <Select label="Type" className="tk-status-select" options={TYPE_OPTIONS} value={issue.type} onChange={(type) => void send({ kind: "update", key: issue.key, type })} />
        </Row>
      </Prop>
      <Links issue={issue} issues={issues} send={send} onOpenIssue={onOpenIssue} />
      <Branch issue={issue} project={project} send={send} />
      <PullRequest issue={issue} send={send} onOpenUrl={onOpenUrl} />
      <Prop label="Commits" wide empty={!commits.length}>
        {commits.length ? (
          <Row gap="s" wrap>
            {commits.map((c) => <Badge key={c} variant="secondary" className="tk-sha">{c}</Badge>)}
          </Row>
        ) : (
          <Text as="span" tone="muted" size="small">None</Text>
        )}
      </Prop>
      <Prop label="Sessions" wide empty={!issue.sessions.length}>
        {issue.sessions.length === 0 ? (
          <Text as="span" tone="muted" size="small">None yet</Text>
        ) : (
          <ul className="tk-sessions" aria-label="Claude sessions">
            {[...issue.sessions].sort((a, b) => b.seen - a.seen).map((s) => (
              <li key={s.id} className="tk-session">
                <Row gap="s">
                  <Text as="span" size="small" truncate>{s.name || "Claude session"}</Text>
                  {s.id === last?.id ? <Badge variant="outline">Latest</Badge> : null}
                </Row>
                <Text as="span" tone="muted" size="tiny">
                  <span className="tk-sha" title={s.id}>{s.id.slice(0, 8)}</span> · <Time iso={iso(s.seen)} prefix="seen " />
                </Text>
              </li>
            ))}
          </ul>
        )}
      </Prop>
      <Prop label="Created">
        <Text as="span" size="small"><Time iso={iso(issue.createdAt)} /></Text>
      </Prop>
      <Prop label="Updated">
        <Text as="span" size="small"><Time iso={iso(issue.updatedAt)} /></Text>
      </Prop>
    </aside>
  );
}

/** A single line that grows into a note box when you focus it, with Add note once there is text. */
function Composer({ issue, send }: { issue: Issue; send: Send }) {
  const [note, setNote] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!note.trim() || busy) return;
    setBusy(true);
    try {
      await send({ kind: "note", key: issue.key, text: note });
      setNote("");
      setOpen(false);
    } finally {
      setBusy(false);
    }
  };
  const grown = open || note !== "";
  return (
    <form className="tk-composer" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      {grown ? (
        <textarea
          autoFocus
          aria-label="Note"
          rows={3}
          maxLength={LIMITS.note}
          value={note}
          placeholder="What was done, what's next"
          onChange={(e) => setNote(e.currentTarget.value)}
          onBlur={() => setOpen(false)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && !note) setOpen(false);
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
          }}
          className="tk-textarea"
        />
      ) : (
        <TextInput label="Note" placeholder="Add a note…" maxLength={LIMITS.note} value="" onChange={() => {}} onFocus={() => setOpen(true)} />
      )}
      {note.trim() ? (
        <Row justify="end">
          <Button type="submit" size="sm" busy={busy}>Add note</Button>
        </Row>
      ) : null}
    </form>
  );
}

function Activity({ issue, send }: { issue: Issue; send: Send }) {
  // Newest first, like the rest of the app. The log itself is oldest first.
  const entries: LogEntry[] = [...issue.log].reverse();
  return (
    <Stack gap="m" as="section" aria-label="Activity">
      <Heading actions={<Text tone="muted" size="small">{issue.log.length} {issue.log.length === 1 ? "entry" : "entries"}</Text>}>Activity</Heading>
      <Composer issue={issue} send={send} />
      <Timeline label={`Activity on ${issue.key}, newest first`}>
        {entries.map((e, i) =>
          e.kind === "note" ? (
            <TimelineCard key={`${e.at}-${i}`} author={actorName(e.by)} at={iso(e.at)} tag={!isClaude(e.by) || !e.by.name ? undefined : <Badge variant="outline">Claude</Badge>} clamp>
              <Markdown source={e.text} />
            </TimelineCard>
          ) : (
            <TimelineNote key={`${e.at}-${i}`} author={actorName(e.by)} at={iso(e.at)}>{e.text}</TimelineNote>
          ),
        )}
      </Timeline>
    </Stack>
  );
}

export function IssueView({ project, issue, issues, issueKey, error, send, onBack, onResume, onStart, onOpenUrl, onOpenIssue }: IssueViewProps) {
  const [confirming, setConfirming] = useState(false);
  const crumb = (
    <nav className="tk-crumbs" aria-label="Breadcrumb">
      <Button variant="ghost" size="sm" onClick={onBack}><span aria-hidden="true">←</span> Issues</Button>
      <span aria-hidden="true" className="tk-crumb-sep">/</span>
      <span className="tk-crumb">{issue?.key ?? issueKey}</span>
    </nav>
  );
  const toolbar = (
    <Toolbar
      actions={
        issue ? (
          <>
            <Button variant="outline" size="sm" disabled={!lastSession(issue)} onClick={onResume} title={lastSession(issue) ? "Open the latest session in a new terminal" : "No session to resume yet"}>Resume</Button>
            <Button size="sm" onClick={onStart}>Start agent</Button>
            <Menu label="…" aria-label="More" variant="ghost" size="sm" align="end" items={[{ id: "delete", label: "Delete issue", variant: "destructive", onSelect: () => setConfirming(true) }]} />
          </>
        ) : null
      }
    >
      {crumb}
    </Toolbar>
  );

  if (issue === undefined) return <Page toolbar={toolbar}><Loading>Loading issue</Loading></Page>;
  if (issue === null) {
    return (
      <Page toolbar={toolbar}>
        <Empty title={`No issue ${issueKey}`} description={`${project.name} has no issue with that key. It may have been removed.`}>
          <Button variant="outline" onClick={onBack}>Back to the board</Button>
        </Empty>
      </Page>
    );
  }

  return (
    <Page toolbar={toolbar} className="tk-page">
      <div className="tk-wrap">
        {error || confirming ? (
          <Stack gap="m" className="tk-notices">
            {error ? <Card className="tk-error-card"><Text tone="danger" role="alert">{error}</Text></Card> : null}
            {confirming ? (
              <Card>
                <Row gap="m" wrap justify="between">
                  <Text size="small">Delete {issue.key}? This removes the issue and its log. Its key is never used again.</Text>
                  <Row gap="s">
                    <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>Keep it</Button>
                    <Button variant="destructive" size="sm" onClick={() => void send({ kind: "remove", key: issue.key })}>Delete {issue.key}</Button>
                  </Row>
                </Row>
              </Card>
            ) : null}
          </Stack>
        ) : null}
        <div className="tk-grid">
          <header className="tk-head">
            <IssueTitle issue={issue} send={send} />
          </header>
          <Properties issue={issue} issues={issues} project={project} send={send} onOpenUrl={onOpenUrl} onOpenIssue={onOpenIssue} />
          <div className="tk-main">
            <Description issue={issue} send={send} />
            {canParent(issue.type) ? <Children issue={issue} issues={issues} onOpenIssue={onOpenIssue} /> : null}
            <Activity issue={issue} send={send} />
          </div>
        </div>
      </div>
    </Page>
  );
}
