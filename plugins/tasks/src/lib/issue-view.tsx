// The Issue tab without its data: one issue with its spec, status, links, sessions and activity. The tab page feeds it
// from storage, a fixture from made-up data.

import { useState } from "react";
import { Badge, Button, Card, Empty, Field, Heading, List, ListItem, Loading, Markdown, Page, Row, Select, Stack, Text, TextInput, Time, Timeline, TimelineCard, TimelineNote, Toolbar } from "wings-plugin-ui";
import { actorName, LIMITS, lastSession, STATUS_LABEL, STATUSES, type Issue, type LogEntry, type Op, type Status } from "./issues";

export interface IssueViewProps {
  project: WingsProject;
  /** `undefined` while it loads, `null` when the project has no issue with that key. */
  issue: Issue | null | undefined;
  /** The key the tab was opened for, for the not-found page. */
  issueKey: string;
  error: string | null;
  send: (op: Op) => Promise<void>;
  onBack: () => void;
  onResume: () => void;
  onStart: () => void;
  onOpenUrl: (url: string) => void;
}

const STATUS_OPTIONS = STATUSES.map((value) => ({ value, label: STATUS_LABEL[value] }));
const iso = (ms: number) => new Date(ms).toISOString();

/** A one-line input that keeps its text while you type and saves on Enter or when you leave it. The page remounts it (`key`) when the saved value changes. */
function SavedInput({ label, value, max, placeholder, required, onSave }: { label: string; value: string; max: number; placeholder?: string; required?: boolean; onSave: (value: string) => void }) {
  const [draft, setDraft] = useState(value);
  const save = () => {
    const next = draft.trim();
    if (next === value || (required && !next)) return setDraft(value);
    onSave(next);
  };
  return (
    <Field label={label}>
    <TextInput
      value={draft}
      maxLength={max}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={save}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
        if (e.key === "Escape") setDraft(value);
      }}
    />
    </Field>
  );
}

function Description({ issue, send }: { issue: Issue; send: (op: Op) => Promise<void> }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const edit = () => {
    setDraft(issue.description ?? "");
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
  return (
    <Stack gap="s">
      <Heading actions={editing ? null : <Button size="xs" variant="outline" onClick={edit}>{issue.description ? "Edit" : "Write"}</Button>}>Description</Heading>
      {editing ? (
        <Stack gap="s">
          <textarea aria-label="Description" rows={12} maxLength={LIMITS.description} value={draft} placeholder="Goal, scope and acceptance criteria. Markdown works." onChange={(e) => setDraft(e.currentTarget.value)} className="tk-textarea" />
          <Row gap="s" justify="end">
            <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>Cancel</Button>
            <Button size="sm" busy={busy} onClick={() => void save()}>Save</Button>
          </Row>
        </Stack>
      ) : issue.description ? (
        <Markdown source={issue.description} />
      ) : (
        <Text tone="muted">No description yet. Write the goal, the scope and what done looks like, so anyone can pick this up later.</Text>
      )}
    </Stack>
  );
}

function Links({ issue, project, send, onOpenUrl }: { issue: Issue; project: WingsProject; send: (op: Op) => Promise<void>; onOpenUrl: (url: string) => void }) {
  const l = issue.links;
  return (
    <Stack gap="s">
      <Heading>Links</Heading>
      <Row gap="s" wrap align="start" className="tk-links">
        <div className="tk-link-field">
          <SavedInput key={`b-${l?.branch ?? ""}`} label="Branch" value={l?.branch ?? ""} max={LIMITS.branch} placeholder="No branch" onSave={(branch) => void send({ kind: "update", key: issue.key, branch })} />
          {project.branch && project.branch !== l?.branch ? (
            <Button size="xs" variant="ghost" onClick={() => void send({ kind: "update", key: issue.key, branch: project.branch ?? "" })}>Use {project.branch}</Button>
          ) : null}
        </div>
        <div className="tk-link-field">
          <SavedInput key={`p-${l?.pr ?? ""}`} label="Pull request" value={l?.pr ?? ""} max={LIMITS.pr} placeholder="https://" onSave={(pr) => void send({ kind: "update", key: issue.key, pr })} />
          {l?.pr ? <Button size="xs" variant="ghost" onClick={() => onOpenUrl(l.pr!)}>Open</Button> : null}
        </div>
      </Row>
      {l?.commits?.length ? (
        <Row gap="s" wrap>
          <Text tone="muted" size="small">Commits</Text>
          {l.commits.map((c) => <Badge key={c} variant="secondary" className="tk-sha">{c}</Badge>)}
        </Row>
      ) : null}
    </Stack>
  );
}

function Sessions({ issue }: { issue: Issue }) {
  const last = lastSession(issue);
  return (
    <Stack gap="s">
      <Heading>Sessions</Heading>
      {issue.sessions.length === 0 ? (
        <Text tone="muted">No Claude session has worked on this yet. Start an agent above, or ask one to pick it up with tasks__get.</Text>
      ) : (
        <List compact aria-label="Claude sessions">
          {[...issue.sessions].sort((a, b) => b.seen - a.seen).map((s) => (
            <ListItem key={s.id} className="tk-session">
              <Text as="span" grow truncate>{s.name || "Claude session"}</Text>
              {s.id === last?.id ? <Badge variant="outline">Latest</Badge> : null}
              <Text as="span" mono tone="muted" size="tiny" title={s.id}>{s.id.slice(0, 8)}</Text>
              <Time iso={iso(s.seen)} prefix="Seen " />
            </ListItem>
          ))}
        </List>
      )}
    </Stack>
  );
}

function Activity({ issue, send }: { issue: Issue; send: (op: Op) => Promise<void> }) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!note.trim() || busy) return;
    setBusy(true);
    try {
      await send({ kind: "note", key: issue.key, text: note });
      setNote("");
    } finally {
      setBusy(false);
    }
  };
  // Newest first, like the rest of the app. The log itself is oldest first.
  const entries: LogEntry[] = [...issue.log].reverse();
  return (
    <Stack gap="s">
      <Heading actions={<Text tone="muted" size="small">{issue.log.length} {issue.log.length === 1 ? "entry" : "entries"}</Text>}>Activity</Heading>
      <Card as="form" className="tk-composer" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <textarea aria-label="Note" rows={3} maxLength={LIMITS.note} value={note} placeholder="What was done, what's next" onChange={(e) => setNote(e.currentTarget.value)} className="tk-textarea" />
        <Row justify="end">
          <Button type="submit" size="sm" busy={busy} disabled={!note.trim()}>Add note</Button>
        </Row>
      </Card>
      <Timeline label={`Activity on ${issue.key}, newest first`}>
        {entries.map((e, i) =>
          e.kind === "note" ? (
            <TimelineCard key={`${e.at}-${i}`} author={actorName(e.by)} at={iso(e.at)} tag={e.by === "you" || !e.by.name ? undefined : <Badge variant="outline">Claude</Badge>} clamp>
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

export function IssueView({ project, issue, issueKey, error, send, onBack, onResume, onStart, onOpenUrl }: IssueViewProps) {
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
        <Stack gap="l">
          {error ? <Card className="tk-error-card"><Text tone="danger" role="alert">{error}</Text></Card> : null}
          <Stack gap="s" as="header">
            <SavedInput key={`t-${issue.title}`} label="Title" value={issue.title} max={LIMITS.title} required onSave={(title) => void send({ kind: "update", key: issue.key, title })} />
            <Text tone="muted" size="small">
              <Time iso={iso(issue.createdAt)} prefix="Created " />, <Time iso={iso(issue.updatedAt)} prefix="updated " />
            </Text>
            <Field label="Status" className="tk-status-field">
              <Select options={STATUS_OPTIONS} value={issue.status} onChange={(status: Status) => void send({ kind: "update", key: issue.key, status })} />
            </Field>
          </Stack>
          <Description issue={issue} send={send} />
          <Links issue={issue} project={project} send={send} onOpenUrl={onOpenUrl} />
          <Sessions issue={issue} />
          <Activity issue={issue} send={send} />
          <Row gap="m" wrap justify="between" className="tk-danger">
            <Text tone="muted" size="small">Deleting removes the issue and its log. Its key is never used again.</Text>
            {confirming ? (
              <Row gap="s">
                <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>Keep it</Button>
                <Button variant="destructive" size="sm" onClick={() => void send({ kind: "remove", key: issue.key })}>Delete {issue.key}</Button>
              </Row>
            ) : (
              <Button variant="destructive" size="sm" onClick={() => setConfirming(true)}>Delete issue</Button>
            )}
          </Row>
        </Stack>
      </div>
    </Page>
  );
}
