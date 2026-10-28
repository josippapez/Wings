// The Schedules tab: the saved schedules with what each does next and last did, and the form to add or change one.

import { useState } from "react";
import { Badge, Button, Card, Confirm, Empty, Icon, ListItem, List, Loading, Notice, Page, Row, Stack, Text, Time, Toolbar, useAction } from "wings-plugin-ui";
import type { State } from "../lib/messages";
import { AGENTS, describeNext, describeWhen, nameOf, nextRun, type Draft, type Schedule } from "../lib/schedule";
import { ScheduleForm } from "./ScheduleForm";

export interface ViewProps {
  /** Null while it loads. */
  state: State | null;
  /** Why it couldn't load. */
  error: string | null;
  now: number;
  onSave: (id: string | null, draft: Draft) => Promise<unknown>;
  onToggle: (id: string) => Promise<unknown>;
  onRemove: (id: string) => Promise<unknown>;
  onRun: (id: string) => Promise<unknown>;
}

export function ScheduleView({ state, error, now, onSave, onToggle, onRemove, onRun }: ViewProps) {
  // `false` is the list, `null` a new schedule, else the schedule being changed.
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

  const { schedules, projects } = state;
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "Missing project";

  if (editing !== false) {
    const schedule = schedules.find((s) => s.id === editing) ?? null;
    return (
      <Page
        toolbar={
          <Toolbar title="Schedules">
            <Button variant="ghost" onClick={() => (action.clear(), setEditing(false))}>
              ← Schedules
            </Button>
            <Text tone="muted">{schedule ? nameOf(schedule) : "New schedule"}</Text>
          </Toolbar>
        }
      >
        <ScheduleForm
          key={schedule?.id ?? "new"}
          schedule={schedule}
          projects={projects}
          busy={action.busy}
          error={action.error}
          onSave={(draft) => void action.run(() => onSave(schedule?.id ?? null, draft)).then((ok) => ok && setEditing(false))}
          onCancel={() => (action.clear(), setEditing(false))}
        />
      </Page>
    );
  }

  const doomed = schedules.find((s) => s.id === deleting);
  const add = (
    <Button onClick={() => (action.clear(), setEditing(null))}>
      Add schedule
    </Button>
  );

  return (
    <Page toolbar={<Toolbar title="Schedules" actions={schedules.length ? add : undefined} />}>
      <Stack gap="l">
        {action.error ? <Notice tone="danger">{action.error}</Notice> : null}
        {error ? <Notice tone="warn">{error}</Notice> : null}
        {doomed ? (
          <Confirm
            title={`Delete ${nameOf(doomed)}?`}
            onCancel={() => setDeleting(null)}
            actions={[{ id: "delete", label: "Delete", onConfirm: () => onRemove(doomed.id).then(() => setDeleting(null)) }]}
          >
            It won't run again. Tabs it already started stay open.
          </Confirm>
        ) : null}
        {schedules.length === 0 ? (
          <Empty title="No schedules yet" description="Pick a project and a prompt, and Wings starts the agent for you at the time you choose. Wings has to be running.">
            {add}
          </Empty>
        ) : (
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
          </List>
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

function LastRun({ run, now }: { run: Schedule["lastRun"]; now: number }) {
  if (!run) return <Text tone="muted" size="small">Not run yet</Text>;
  return (
    <Row gap="s" wrap>
      <Text tone="muted" size="small" as="span">
        <Time iso={new Date(run.at).toISOString()} prefix="Last run " now={now} />
      </Text>
      {run.ok ? <Badge size="sm" tone="success">Started</Badge> : <Badge size="sm" tone="danger">Failed</Badge>}
      {run.ok ? null : <Text tone="danger" size="small">{run.error}</Text>}
    </Row>
  );
}
