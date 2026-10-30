// The Schedules tab: what is waiting for you, the saved entries with what each does next and last did, and the form to add or change one.

import { useState } from "react";
import { Badge, Button, Card, Confirm, Disclosure, Empty, Heading, Icon, ListItem, List, Loading, Menu, Notice, Page, Row, Stack, Text, Time, Toolbar, useAction } from "wings-plugin-ui";
import { asks, describeCheck, SOURCE_LABELS, startedElsewhere, titleWanted, watchName, type AssignedTicket, type Waiting, type WatchDraft } from "../lib/assigned";
import type { State, WatchView } from "../lib/messages";
import type { WorktreeChange, WorktreeSetup } from "../lib/worktree-setup";
import { AGENTS, describeNext, describeWhen, nameOf, nextRun, type Draft, type Run, type Schedule } from "../lib/schedule";
import { ScheduleForm } from "./ScheduleForm";
import "./schedules.css";

/** The ticket a notification's Start button asked for, and how starting it went. */
export interface InboxStart {
  entry: string;
  ticket: string;
  status: "starting" | "started" | "failed";
  error?: string;
}

export interface ViewProps {
  /** Null while it loads. */
  state: State | null;
  /** Why it couldn't load. */
  error: string | null;
  now: number;
  /** The project the tab is in: the page shows its entries, until you ask for all. */
  here?: string | null;
  /** The entry a notification sent you here for: its waiting tickets come first. */
  focusWatch?: string | null;
  inbox?: InboxStart | null;
  onSave: (id: string | null, draft: Draft) => Promise<unknown>;
  onToggle: (id: string) => Promise<unknown>;
  onRemove: (id: string) => Promise<unknown>;
  onRun: (id: string) => Promise<unknown>;
  onSaveWatch: (id: string | null, draft: WatchDraft, worktree?: { projectId: string; change: WorktreeChange }) => Promise<unknown>;
  onToggleWatch: (id: string) => Promise<unknown>;
  onRemoveWatch: (id: string) => Promise<unknown>;
  onCheckWatch: (id: string) => Promise<unknown>;
  onStart: (id: string, ticketId: string, force?: boolean, projectId?: string) => Promise<unknown>;
  onDismiss: (id: string, ticketId: string) => Promise<unknown>;
  onSignIn: (id: string) => Promise<unknown>;
  onInstall: (id: string) => Promise<unknown>;
  onOpenUrl: (url: string) => Promise<unknown>;
  loadBranches?: (projectId: string) => Promise<string[]>;
  loadWorktreeSetup?: (projectId: string) => Promise<WorktreeSetup>;
}

export function ScheduleView({ state, error, now, here = null, focusWatch, inbox, onSave, onToggle, onRemove, onRun, onSaveWatch, onToggleWatch, onRemoveWatch, onCheckWatch, onStart, onDismiss, onSignIn, onInstall, onOpenUrl, loadBranches, loadWorktreeSetup }: ViewProps) {
  // `false` is the list, `null` a new entry, else the schedule or "assigned" entry being changed.
  const [editing, setEditing] = useState<string | null | false>(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const action = useAction();
  const [checking, setChecking] = useState<string | null>(null);
  const [everywhere, setEverywhere] = useState(false);

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

  const { projects, signInCode } = state;
  const inScope = (projectId: string, id: string) => everywhere || !here || projectId === here || id === focusWatch;
  const schedules = state.schedules.filter((s) => inScope(s.projectId, s.id));
  const watches = state.watches.filter((w) => inScope(w.projectId, w.id));
  const elsewhere = state.schedules.length + state.watches.length - schedules.length - watches.length;
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "Missing project";

  if (editing !== false) {
    const schedule = state.schedules.find((s) => s.id === editing) ?? null;
    const watch = state.watches.find((w) => w.id === editing) ?? null;
    return (
      <Page
        toolbar={
          <Toolbar title="Schedules">
            <Button variant="ghost" onClick={() => (action.clear(), setEditing(false))}>
              ← Schedules
            </Button>
            <Text tone="muted">{schedule ? nameOf(schedule) : watch ? watchName(watch) : "New entry"}</Text>
          </Toolbar>
        }
      >
        <ScheduleForm
          key={editing || "new"}
          schedule={schedule}
          watch={watch}
          projects={projects}
          here={here}
          loadBranches={loadBranches}
          loadWorktreeSetup={loadWorktreeSetup}
          busy={action.busy}
          error={action.error}
          onSave={(draft) => void action.run(() => onSave(schedule?.id ?? null, draft)).then((ok) => ok && setEditing(false))}
          onSaveWatch={(draft, worktree) => void action.run(() => onSaveWatch(watch?.id ?? null, draft, worktree)).then((ok) => ok && setEditing(false))}
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
  const scope =
    here && (everywhere || elsewhere > 0) ? (
      <Button variant="ghost" onClick={() => setEverywhere(!everywhere)}>
        {everywhere ? "This project" : `All projects (${elsewhere} more)`}
      </Button>
    ) : null;

  return (
    <Page
      toolbar={
        <Toolbar
          title="Schedules"
          actions={
            empty && !scope ? undefined : (
              <Row gap="s">
                {scope}
                {empty ? null : add}
              </Row>
            )
          }
        />
      }
    >
      <Stack gap="l">
        {action.error ? <Notice tone="danger">{action.error}</Notice> : null}
        {error ? <Notice tone="warn">{error}</Notice> : null}
        {inbox ? (
          <Notice tone={inbox.status === "failed" ? "danger" : inbox.status === "started" ? "success" : "info"}>
            {inbox.status === "failed" ? inbox.error : `${inbox.status === "started" ? "Started" : "Starting"} ${ticketTitle(watches.find((w) => w.id === inbox.entry), inbox.ticket)}.`}
          </Notice>
        ) : null}
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
          <Empty title={elsewhere > 0 ? "Nothing set up for this project" : "Nothing set up yet"} description="Pick a project and a prompt, and Wings starts the agent for you at the time you choose, or when a ticket is assigned to you. Wings has to be running.">
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
                  checking={checking === w.id}
                  onCheck={() => (setChecking(w.id), void action.run(() => onCheckWatch(w.id)).finally(() => setChecking(null)))}
                  onStartTicket={(ticketId, force, projectId) => void action.run(() => onStart(w.id, ticketId, force, projectId))}
                  onSignIn={() => void action.run(() => onSignIn(w.id))}
                  onInstall={() => void action.run(() => onInstall(w.id))}
                  onOpen={(url) => void action.run(() => onOpenUrl(url))}
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

interface WatchItemProps {
  w: WatchView;
  project: string;
  now: number;
  busy: boolean;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
  checking: boolean;
  onCheck: () => void;
  onStartTicket: (ticketId: string, force: boolean, projectId?: string) => void;
  onSignIn: () => void;
  onInstall: () => void;
  onOpen: (url: string) => void;
}

function WatchItem({ w, project, now, busy, onEdit, onToggle, onDelete, checking, onCheck, onStartTicket, onSignIn, onInstall, onOpen }: WatchItemProps) {
  const check = w.lastCheck;
  const label = `${watchName(w)} in ${project}`;
  const unsupported = w.source?.kind === "unsupported";
  const tickets = w.tickets ?? [];
  return (
    <ListItem align="start">
      <Card className="sc-item">
        <Stack gap="m">
        <Row gap="l" justify="between" wrap grow>
          <Stack gap="xs" grow>
            <Row gap="m" wrap>
              <Text weight="strong" truncate>
                {watchName(w)}
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
            {w.enabled && !unsupported ? (
              <Button variant="outline" size="sm" onClick={onCheck} disabled={busy} busy={checking} aria-label={`${check && !check.ok ? "Try" : "Check"} ${label} ${check && !check.ok ? "again" : "now"}`}>
                {check && !check.ok ? "Try again" : "Check now"}
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
        {tickets.length ? (
          <Disclosure summary={`Assigned to you (${tickets.length})`}>
            <List compact>
              {tickets.map((t) => (
                <TicketRow key={t.id} t={t} asked={asks(w, t)} titleOk={titleWanted(w.prefixes, t.title)} startedAt={w.started?.[t.id]} now={now} busy={busy} targets={w.targets} onStart={(projectId) => onStartTicket(t.id, !asks(w, t) || !!startedElsewhere(t) || !!w.started?.[t.id], projectId)} onOpen={() => onOpen(t.url)} />
              ))}
            </List>
          </Disclosure>
        ) : null}
        </Stack>
      </Card>
    </ListItem>
  );
}

interface TicketRowProps {
  t: AssignedTicket;
  asked: boolean;
  /** False when the entry wants other title prefixes: the ticket is for another project. */
  titleOk: boolean;
  /** When Wings started an agent on it. */
  startedAt: number | undefined;
  now: number;
  busy: boolean;
  /** The projects it can start in: with more than one, Start asks which. */
  targets: { id: string; name: string }[];
  onStart: (projectId?: string) => void;
  onOpen: () => void;
}

function TicketRow({ t, asked, titleOk, startedAt, now, busy, targets, onStart, onOpen }: TicketRowProps) {
  const label = startedAt ? "Start again" : asked && !startedElsewhere(t) ? "Start" : "Start anyway";
  const variant = label === "Start" ? "default" : "outline";
  return (
    <ListItem align="start" className="sc-ticket">
      <Row gap="m" justify="between" wrap grow>
        <Stack gap="xs" grow>
          <Text tone={asked ? undefined : "muted"} truncate>
            {t.title}
          </Text>
          <Text tone="muted" size="small">
            #{t.id}
            {t.type ? ` · ${t.type}` : ""}
          </Text>
          {startedElsewhere(t) ? (
            <Text tone="muted" size="small" truncate>
              Started: {startedElsewhere(t)}
            </Text>
          ) : null}
        </Stack>
        <Row gap="s" wrap align="start">
          {asked ? <Badge size="sm" variant="secondary">{t.state}</Badge> : <Badge size="sm" variant="outline">{titleOk ? `Not asked: ${t.state}` : "Other project"}</Badge>}
          <Button variant="ghost" size="sm" onClick={onOpen} aria-label={`Open ${t.title}`}>
            Open
          </Button>
          {startedAt ? (
            <Text tone="muted" size="small" as="span">
              <Time iso={new Date(startedAt).toISOString()} prefix="Started " now={now} />
            </Text>
          ) : null}
          {targets.length > 1 ? (
            <Menu size="sm" variant={variant} align="end" label={label} disabled={busy} aria-label={`${label} ${t.title}`} items={targets.map((p) => ({ id: p.id, label: `In ${p.name}`, onSelect: () => onStart(p.id) }))} />
          ) : (
            <Button size="sm" variant={variant} onClick={() => onStart()} disabled={busy} aria-label={`${label} ${t.title}`}>
              {label}
            </Button>
          )}
        </Row>
      </Row>
    </ListItem>
  );
}

/** A ticket's title from what the entry waits on or listed, else its number. */
const ticketTitle = (w: WatchView | undefined, id: string) => w?.waiting.find((t) => t.id === id)?.title ?? w?.tickets?.find((t) => t.id === id)?.title ?? `#${id}`;

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
