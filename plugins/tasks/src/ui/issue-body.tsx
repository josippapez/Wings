// The main column of the Issue page: the description, an epic's or feature's child issues, and the activity with its note box.

import { useState } from "react";
import { Badge, Button, Disclosure, Heading, List, ListButton, ListItem, Markdown, Meter, Row, Stack, Text, Textarea, TextInput, Time, Timeline, TimelineCard, TimelineNote, useAction } from "wings-plugin-ui";
import { actorName, isAgent } from "../lib/actor";
import { agentLabel } from "../lib/commands";
import { DESCRIPTION_TEMPLATE, keyNumber, LIMITS, STATUS_LABEL, visibleDescription, type FileChange, type Issue, type LogEntry } from "../lib/domain";
import { iso } from "../lib/format";
import { childrenOf, progress } from "../lib/relations";
import { ErrorText, openProps, StatusDot, TypeIcon, type Apply } from "./parts";

export function Description({ issue, apply }: { issue: Issue; apply: Apply }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const { run, busy, error, clear } = useAction();
  const edit = () => {
    setDraft(issue.description || DESCRIPTION_TEMPLATE);
    setEditing(true);
  };
  const save = async () => {
    if (await run(() => apply({ kind: "update", key: issue.key, description: draft }))) setEditing(false);
  };
  const close = () => {
    clear();
    setEditing(false);
  };
  if (editing) {
    return (
      <Stack gap="s">
        <Textarea autoFocus label="Description" rows={12} maxLength={LIMITS.description} value={draft} placeholder="Goal, scope and acceptance criteria. Markdown works." onChange={(e) => setDraft(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Escape") close(); }} />
        {error ? <ErrorText size="small">{error}</ErrorText> : null}
        <Row gap="s" justify="end">
          <Button variant="ghost" size="sm" onClick={close}>Cancel</Button>
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
      {total ? <Meter percent={(done / total) * 100} label="Child issues done" tone="success" /> : null}
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

/** Rows of Files changed shown before the rest fold away. */
const FILE_ROWS = 10;

function FileRow({ file }: { file: FileChange }) {
  return (
    <ListItem>
      <div className="tk-file">
        <Text as="span" mono truncate className="tk-file-path">{file.path}</Text>
        {file.what ? <Text as="span" tone="muted" size="small" className="tk-file-what">{file.what}</Text> : null}
        <Text as="span" tone="muted" size="tiny">
          {file.session ? `${file.session.name || agentLabel(file.session)} · ` : ""}
          <Time iso={iso(file.at)} />
        </Text>
      </div>
    </ListItem>
  );
}

/** The files agents' journals reported for the issue, newest first, with the rows past the first ten folded. */
export function FilesChanged({ issue }: { issue: Issue }) {
  const files = [...(issue.files ?? [])].sort((a, b) => b.at - a.at);
  if (!files.length) return null;
  const rest = files.slice(FILE_ROWS);
  return (
    <Stack gap="s" as="section" aria-label="Files changed">
      <Heading actions={<Text tone="muted" size="small">{files.length}</Text>}>Files changed</Heading>
      <List compact aria-label={`Files changed on ${issue.key}, newest first`}>
        {files.slice(0, FILE_ROWS).map((f) => <FileRow key={f.path} file={f} />)}
      </List>
      {rest.length ? (
        <Disclosure summary={<Text as="span" size="small" tone="muted">{rest.length} more</Text>}>
          <List compact aria-label={`More files changed on ${issue.key}`}>
            {rest.map((f) => <FileRow key={f.path} file={f} />)}
          </List>
        </Disclosure>
      ) : null}
    </Stack>
  );
}

/** A single line that grows into a note box when you focus it, with Add note once there is text. */
function Composer({ issue, apply }: { issue: Issue; apply: Apply }) {
  const [note, setNote] = useState("");
  const [open, setOpen] = useState(false);
  const { run, busy, error, clear } = useAction();
  const submit = () => {
    if (!note.trim() || busy) return;
    void run(async () => {
      await apply({ kind: "note", key: issue.key, text: note });
      setNote("");
      setOpen(false);
    });
  };
  const grown = open || note !== "";
  return (
    <form className="tk-composer" onSubmit={(e) => { e.preventDefault(); submit(); }}>
      {grown ? (
        <Textarea
          autoFocus
          label="Note"
          rows={3}
          maxLength={LIMITS.note}
          value={note}
          placeholder="What was done, what's next"
          onChange={(e) => {
            setNote(e.currentTarget.value);
            clear();
          }}
          onBlur={() => setOpen(false)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && !note) setOpen(false);
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
          }}
        />
      ) : (
        <TextInput label="Note" placeholder="Add a note…" maxLength={LIMITS.note} value="" onChange={() => {}} onFocus={() => setOpen(true)} />
      )}
      {error ? <ErrorText size="small">{error}</ErrorText> : null}
      {note.trim() ? (
        <Row justify="end">
          <Button type="submit" size="sm" busy={busy}>Add note</Button>
        </Row>
      ) : null}
    </form>
  );
}

export function Activity({ issue, apply }: { issue: Issue; apply: Apply }) {
  // Newest first, like the rest of the app. The log itself is oldest first.
  const entries: LogEntry[] = [...issue.log].reverse();
  return (
    <Stack gap="m" as="section" aria-label="Activity">
      <Heading actions={<Text tone="muted" size="small">{issue.log.length} {issue.log.length === 1 ? "entry" : "entries"}</Text>}>Activity</Heading>
      <Composer issue={issue} apply={apply} />
      <Timeline label={`Activity on ${issue.key}, newest first`}>
        {entries.map((e, i) =>
          e.kind === "note" ? (
            <TimelineCard key={`${e.at}-${i}`} author={actorName(e.by)} at={iso(e.at)} tag={!isAgent(e.by) || !e.by.name ? undefined : <Badge variant="outline">{agentLabel(e.by)}</Badge>} clamp>
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
