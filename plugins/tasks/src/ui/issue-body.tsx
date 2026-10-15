// The main column of the Issue page: the description, an epic's or feature's child issues, and the activity with its note box.

import { useState } from "react";
import { Badge, Button, Heading, List, ListButton, ListItem, Markdown, Row, Stack, Text, TextInput, Timeline, TimelineCard, TimelineNote, useAction } from "wings-plugin-ui";
import { actorName, isClaude } from "../lib/actor";
import { DESCRIPTION_TEMPLATE, keyNumber, LIMITS, STATUS_LABEL, visibleDescription, type Issue, type LogEntry } from "../lib/domain";
import { iso } from "../lib/format";
import { childrenOf, progress } from "../lib/relations";
import { openProps, StatusDot, TypeIcon, type Send } from "./parts";

export function Description({ issue, send }: { issue: Issue; send: Send }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const { run, busy } = useAction();
  const edit = () => {
    setDraft(issue.description || DESCRIPTION_TEMPLATE);
    setEditing(true);
  };
  const save = () =>
    run(async () => {
      await send({ kind: "update", key: issue.key, description: draft });
      setEditing(false);
    });
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

/** An epic's or feature's children with how many are done. */
export function Children({ issue, issues, onOpenIssue }: { issue: Issue; issues: Issue[]; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const children = childrenOf(issues, issue.key).sort((a, b) => keyNumber(a.key) - keyNumber(b.key));
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

/** A single line that grows into a note box when you focus it, with Add note once there is text. */
function Composer({ issue, send }: { issue: Issue; send: Send }) {
  const [note, setNote] = useState("");
  const [open, setOpen] = useState(false);
  const { run, busy } = useAction();
  const submit = () => {
    if (!note.trim() || busy) return;
    void run(async () => {
      await send({ kind: "note", key: issue.key, text: note });
      setNote("");
      setOpen(false);
    });
  };
  const grown = open || note !== "";
  return (
    <form className="tk-composer" onSubmit={(e) => { e.preventDefault(); submit(); }}>
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
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
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

export function Activity({ issue, send }: { issue: Issue; send: Send }) {
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
