// The Schedules tab: what is waiting for you, the saved entries with what each does next and last did, and the form to add or change one.

import { useState } from "react";
import { Badge, Button, Card, Confirm, Empty, Heading, Icon, ListItem, List, Loading, Notice, Page, Row, Stack, Text, Time, Toolbar, useAction } from "wings-plugin-ui";
import { describeCheck, SOURCE_LABELS, type Waiting, type WatchDraft } from "../lib/assigned";
import type { State, WatchView } from "../lib/messages";
import { AGENTS, describeNext, describeWhen, nameOf, nextRun, type Draft, type Run, type Schedule } from "../lib/schedule";
import { ScheduleForm } from "./ScheduleForm";
import "./schedules.css";

export interface ViewProps {
  /** Null while it loads. */
  state: State | null;
  /** Why it couldn't load. */
  error: string | null;
  now: number;
  /** The entry a notification sent you here for: its waiting tickets come first. */
  focusWatch?: string | null;
  onSave: (id: string | null, draft: Draft) => Promise<unknown>;
  onToggle: (id: string) => Promise<unknown>;
  onRemove: (id: string) => Promise<unknown>;
  onRun: (id: string) => Promise<unknown>;
  onSaveWatch: (id: string | null, draft: WatchDraft) => Promise<unknown>;
  onToggleWatch: (id: string) => Promise<unknown>;
  onRemoveWatch: (id: string) => Promise<unknown>;
  onCheckWatch: (id: string) => Promise<unknown>;
  onStart: (id: string, ticketId: string) => Promise<unknown>;
  onDismiss: (id: string, ticketId: string) => Promise<unknown>;
  onSignIn: (id: string) => Promise<unknown>;
  onInstall: (id: string) => Promise<unknown>;
  onOpenUrl: (url: string) => Promise<unknown>;
}

export function ScheduleView({ state, error, now, focusWatch, onSave, onToggle, onRemove, onRun, onSaveWatch, onToggleWatch, onRemoveWatch, onCheckWatch, onStart, onDismiss, onSignIn, onInstall, onOpenUrl }: ViewProps) {
  // `false` is the list, `null` a new entry, else the schedule or "assigned" entry being changed.
  const [editing, setEditing] = useState<string | null | false>(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const action = useAction();

  if (error && !state) {
    return (
      <Page toolbar={<Toolbar title="Schedules" />}>
        <Empty title="Couldn't load your schedules" description={error} />
      </Page>
    );
  }
  if (!state) {
    return (
      <Page toolbar={<Toolbar title="Schedules" />}>
        <Loading>Loading schedules</Loading>
      </Page>
    );
  }

  const { schedules, watches, projects, signInCode } = state;
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "Missing project";

  if (editing !== false) {
    const schedule = schedules.find((s) => s.id === editing) ?? null;
    const watch = watches.find((w) => w.id === editing) ?? null;
    return (
      <Page
        toolbar={
          <Toolbar title="Schedules">
            <Button variant="ghost" onClick={() => (action.clear(), setEditing(false))}>
              ← Schedules
            </Button>
            <Text tone="muted">{schedule ? nameOf(schedule) : watch ? "When something is assigned to me" : "New entry"}</Text>
          </Toolbar>
        }
      >
        <ScheduleForm
          key={editing || "new"}
          schedule={schedule}
          watch={watch}
          projects={projects}
          busy={action.busy}
          error={action.error}
          onSave={(draft) => void action.run(() => onSave(schedule?.id ?? null, draft)).then((ok) => ok && setEditing(false))}
          onSaveWatch={(draft) => void action.run(() => onSaveWatch(watch?.id ?? null, draft)).then((ok) => ok && setEditing(false))}
          onCancel={() => (action.clear(), setEditing(false))}
        />
      </Page>
    );
  }

  const doomedSchedule = schedules.find((s) => s.id === deleting);
  const doomedWatch = watches.find((w) => w.id === deleting);
  const add = (
    <Button onClick={() => (action.clear(), setEditing(null))}>
      Add entry
    </Button>
  );
  const waiting = watches
    .flatMap((w) => w.waiting.map((ticket) => ({ w, ticket })))
    .sort((a, b) => Number(b.w.id === focusWatch) - Number(a.w.id === focusWatch));
  const empty = schedules.length === 0 && watches.length === 0;

  return (
    <Page toolbar={<Toolbar title="Schedules" actions={empty ? undefined : add} />}>
      <Stack gap="l">
        {action.error ? <Notice tone="danger">{action.error}</Notice> : null}
        {error ? <Notice tone="warn">{error}</Notice> : null}
        {signInCode ? <Notice>Your GitHub code is {signInCode}. It is on your clipboard: paste it in the browser to finish signing in.</Notice> : null}
        {doomedSchedule || doomedWatch ? (
          <Confirm
            title={`Delete ${doomedSchedule ? nameOf(doomedSchedule) : "this entry"}?`}
            onCancel={() => setDeleting(null)}
            actions={[
              {
                id: "delete",
                label: "Delete",
                onConfirm: () => (doomedSchedule ? onRemove(doomedSchedule.id) : onRemoveWatch(doomedWatch!.id)).then(() => setDeleting(null)),
              },
            ]}
          >
            {doomedSchedule ? "It won't run again. Tabs it already started stay open." : "It stops watching, and what is waiting for you goes with it. Tabs it already started stay open."}
          </Confirm>
        ) : null}
        {waiting.length ? (
          <Stack gap="m" as="section" aria-label="Waiting for you">
            <Heading level={3} className="sc-heading">Waiting for you</Heading>
            <List>
              {waiting.map(({ w, ticket }) => (
                <WaitingItem
                  key={`${w.id}:${ticket.id}`}
                  w={w}
                  ticket={ticket}
                  project={projectName(w.projectId)}
                  busy={action.busy}
                  onStart={() => void action.run(() => onStart(w.id, ticket.id))}
                  onDismiss={() => void action.run(() => onDismiss(w.id, ticket.id))}
                  onOpen={() => void action.run(() => onOpenUrl(ticket.url))}
                />
              ))}
            </List>
          </Stack>
        ) : null}
        {empty ? (
          <Empty title="Nothing set up yet" description="Pick a project and a prompt, and Wings starts the agent for you at the time you choose, or when a ticket is assigned to you. Wings has to be running.">
            {add}
          </Empty>
        ) : (
          <Stack gap="m" as="section" aria-label="Entries">
            {waiting.length ? <Heading level={3} className="sc-heading">Entries</Heading> : null}
            <List>
              {schedules.map((s) => (
                <Item
                  key={s.id}
                  s={s}
                  project={projectName(s.projectId)}
                  now={now}
                  busy={action.busy}
                  onRun={() => void action.run(() => onRun(s.id))}
                  onEdit={() => (action.clear(), setEditing(s.id))}
                  onToggle={() => void action.run(() => onToggle(s.id))}
                  onDelete={() => setDeleting(s.id)}
                />
              ))}
              {watches.map((w) => (
                <WatchItem
                  key={w.id}
                  w={w}
                  project={projectName(w.projectId)}
                  now={now}
                  busy={action.busy}
                  onEdit={() => (action.clear(), setEditing(w.id))}
                  onToggle={() => void action.run(() => onToggleWatch(w.id))}
                  onDelete={() => setDeleting(w.id)}
                  onCheck={() => void action.run(() => onCheckWatch(w.id))}
                  onSignIn={() => void action.run(() => onSignIn(w.id))}
                  onInstall={() => void action.run(() => onInstall(w.id))}
                />
              ))}
            </List>
          </Stack>
        )}
      </Stack>
    </Page>
  );
}

interface ItemProps {
  s: Schedule;
  project: string;
  now: number;
  busy: boolean;
  onRun: () => void;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
}

function Item({ s, project, now, busy, onRun, onEdit, onToggle, onDelete }: ItemProps) {
  const name = nameOf(s);
  const next = nextRun(s);
  return (
    <ListItem align="start">
      <Card className="sc-item">
        <Row gap="l" justify="between" wrap grow>
          <Stack gap="xs" grow>
            <Row gap="m" wrap>
              <Text weight="strong" truncate>
                {name}
              </Text>
              {s.enabled ? null : <Badge size="sm">Paused</Badge>}
            </Row>
            <Text tone="muted" size="small">
              {project} · {AGENTS[s.agent].label} · {describeWhen(s.when)}
            </Text>
            <Text size="small">{s.enabled ? (next === null ? "No day picked" : `Next run ${describeNext(next, now)}`) : "Next run when resumed"}</Text>
            <LastRun run={s.lastRun} now={now} />
          </Stack>
          <Row gap="s" wrap align="start">
            <Button variant="outline" size="sm" onClick={onRun} disabled={busy} aria-label={`Run ${name} now`}>
              Run now
            </Button>
            <Button variant="outline" size="sm" onClick={onEdit} aria-label={`Edit ${name}`}>
              Edit
            </Button>
            <Button variant="outline" size="sm" onClick={onToggle} disabled={busy} aria-label={`${s.enabled ? "Pause" : "Resume"} ${name}`}>
              {s.enabled ? "Pause" : "Resume"}
            </Button>
            <Button variant="ghost" size="icon-sm" onClick={onDelete} aria-label={`Delete ${name}`}>
              <Icon name="trash" />
            </Button>
          </Row>
        </Row>
      </Card>
    </ListItem>
  );
}

const WATCH_NAME = "When something is assigned to me";

interface WatchItemProps {
  w: WatchView;
  project: string;
  now: number;
  busy: boolean;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
  onCheck: () => void;
  onSignIn: () => void;
  onInstall: () => void;
}

function WatchItem({ w, project, now, busy, onEdit, onToggle, onDelete, onCheck, onSignIn, onInstall }: WatchItemProps) {
  const check = w.lastCheck;
  const label = `${WATCH_NAME} in ${project}`;
  const unsupported = w.source?.kind === "unsupported";
  return (
    <ListItem align="start">
      <Card className="sc-item">
        <Row gap="l" justify="between" wrap grow>
          <Stack gap="xs" grow>
            <Row gap="m" wrap>
              <Text weight="strong" truncate>
                {WATCH_NAME}
              </Text>
              {w.enabled ? null : <Badge size="sm">Paused</Badge>}
            </Row>
            <Text tone="muted" size="small">
              {project} · {AGENTS[w.agent].label} · {w.source ? SOURCE_LABELS[w.source.kind] : "Checking the remote"}
            </Text>
            {w.enabled ? (
              <Row gap="s" wrap>
                {check && !unsupported ? (
                  <Text tone="muted" size="small" as="span">
                    <Time iso={new Date(check.at).toISOString()} prefix="Checked " now={now} />
                  </Text>
                ) : null}
                <Text size="small" tone={check && !check.ok ? "danger" : undefined}>
                  {describeCheck(check, w.source)}
                </Text>
              </Row>
            ) : (
              <Text size="small">Not looking while paused</Text>
            )}
            {w.waiting.length ? <Text size="small">{w.waiting.length === 1 ? "1 ticket is waiting for you" : `${w.waiting.length} tickets are waiting for you`}</Text> : null}
            {w.lastRun ? <LastRun run={w.lastRun} now={now} prefix="Last started " /> : null}
          </Stack>
          <Row gap="s" wrap align="start">
            {w.enabled && check?.problem === "signed-out" ? (
              <Button size="sm" onClick={onSignIn} disabled={busy} aria-label={`Sign in for ${label}`}>
                Sign in
              </Button>
            ) : null}
            {w.enabled && check?.problem === "missing" ? (
              <Button size="sm" onClick={onInstall} disabled={busy} aria-label={`Install the CLI for ${label}`}>
                Install
              </Button>
            ) : null}
            {w.enabled && check && !check.ok && !unsupported ? (
              <Button variant="outline" size="sm" onClick={onCheck} disabled={busy} aria-label={`Try ${label} again`}>
                Try again
              </Button>
            ) : null}
            <Button variant="outline" size="sm" onClick={onEdit} aria-label={`Edit ${label}`}>
              Edit
            </Button>
            <Button variant="outline" size="sm" onClick={onToggle} disabled={busy} aria-label={`${w.enabled ? "Pause" : "Resume"} ${label}`}>
              {w.enabled ? "Pause" : "Resume"}
            </Button>
            <Button variant="ghost" size="icon-sm" onClick={onDelete} aria-label={`Delete ${label}`}>
              <Icon name="trash" />
            </Button>
          </Row>
        </Row>
      </Card>
    </ListItem>
  );
}

interface WaitingItemProps {
  w: WatchView;
  ticket: Waiting;
  project: string;
  busy: boolean;
  onStart: () => void;
  onDismiss: () => void;
  onOpen: () => void;
}

function WaitingItem({ w, ticket, project, busy, onStart, onDismiss, onOpen }: WaitingItemProps) {
  return (
    <ListItem align="start">
      <Card className="sc-item">
        <Row gap="l" justify="between" wrap grow>
          <Stack gap="xs" grow>
            <Text weight="strong" truncate>
              {ticket.title}
            </Text>
            <Text tone="muted" size="small">
              #{ticket.id} · {project} · {AGENTS[w.agent].label}
            </Text>
            <Text size="small">Starts a new worktree on a feature branch and opens the agent in it.</Text>
          </Stack>
          <Row gap="s" wrap align="start">
            <Button variant="ghost" size="sm" onClick={onOpen} disabled={busy} aria-label={`Open ${ticket.title}`}>
              Open
            </Button>
            <Button size="sm" onClick={onStart} busy={busy} aria-label={`Start ${ticket.title}`}>
              Start
            </Button>
            <Button variant="outline" size="sm" onClick={onDismiss} disabled={busy} aria-label={`Dismiss ${ticket.title}`}>
              Dismiss
            </Button>
          </Row>
        </Row>
      </Card>
    </ListItem>
  );
}

function LastRun({ run, now, prefix = "Last run " }: { run: Run | undefined; now: number; prefix?: string }) {
  if (!run) return <Text tone="muted" size="small">Not run yet</Text>;
  return (
    <Row gap="s" wrap>
      <Text tone="muted" size="small" as="span">
        <Time iso={new Date(run.at).toISOString()} prefix={prefix} now={now} />
      </Text>
      {run.ok ? <Badge size="sm" tone="success">Started</Badge> : <Badge size="sm" tone="danger">Failed</Badge>}
      {run.ok ? null : <Text tone="danger" size="small">{run.error}</Text>}
    </Row>
  );
}
