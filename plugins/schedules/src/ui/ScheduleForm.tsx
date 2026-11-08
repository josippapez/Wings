// The form that adds or changes a schedule, or an entry that waits for something to be assigned to you.

import { useState } from "react";
import { Button, Card, Chips, Field, FieldGroup, Notice, Row, Select, Stack, TextInput } from "wings-plugin-ui";
import { WATCH_NAME, type WatchDraft } from "../lib/assigned";
import type { WatchView } from "../lib/messages";
import type { WorktreeChange, WorktreeSetup } from "../lib/worktree-setup";
import { AGENTS, type Agent, type Draft, type Schedule } from "../lib/schedule";
import { ScheduleFields, useScheduleFields } from "./ScheduleFields";
import { useWatchFields, WatchFields } from "./WatchFields";
import "./schedules.css";

interface Props {
  /** The schedule or "assigned" entry being changed, or neither for a new one. */
  schedule: Schedule | null;
  watch: WatchView | null;
  projects: { id: string; name: string }[];
  /** The project a new entry starts on: the one the tab is in. */
  here: string | null;
  busy: boolean;
  /** What the last save failed with. */
  error: string | null;
  onSave: (draft: Draft) => void;
  onSaveWatch: (draft: WatchDraft, worktree?: { projectId: string; change: WorktreeChange }) => void;
  onCancel: () => void;
  /** The project's branches, for the branch to start from. */
  loadBranches?: (projectId: string) => Promise<string[]>;
  /** The project's worktree setting, which Wings keeps. */
  loadWorktreeSetup?: (projectId: string) => Promise<WorktreeSetup>;
}

export function ScheduleForm({ schedule, watch, projects, here, busy, error, onSave, onSaveWatch, onCancel, loadBranches, loadWorktreeSetup }: Props) {
  // An entry keeps its kind: only a new one picks it.
  const [kind, setKind] = useState<"schedule" | "assigned">(watch ? "assigned" : "schedule");
  const [name, setName] = useState(schedule?.name ?? watch?.name ?? "");
  const [projectId, setProjectId] = useState(schedule?.projectId ?? watch?.projectId ?? here ?? projects[0]?.id ?? "");
  const [agent, setAgent] = useState<Agent>(schedule?.agent ?? watch?.agent ?? "claude");
  // Both kinds keep their fields here, so a new entry that tries the other kind and comes back finds what it typed.
  const scheduleFields = useScheduleFields(schedule);
  const watchFields = useWatchFields(watch, kind === "assigned", projectId, loadBranches, loadWorktreeSetup);

  // The Save button runs it on a click, which also covers Enter in a field, and stops the form from submitting a second time.
  const submit = (e: { preventDefault(): void }) => {
    e.preventDefault();
    if (kind === "assigned") return onSaveWatch(...watchFields.draft({ name, projectId, agent }));
    onSave({ name, projectId, agent, ...scheduleFields.draft() });
  };

  return (
    <Card as="form" onSubmit={submit} aria-label={schedule || watch ? "Edit entry" : "New entry"}>
      <Stack gap="l">
        {schedule || watch ? null : (
          <FieldGroup label="Start an agent">
            <Chips
              label="Start an agent"
              value={kind}
              onChange={setKind}
              options={[
                { value: "schedule", label: "On a schedule" },
                { value: "assigned", label: "When something is assigned to me" },
              ]}
            />
          </FieldGroup>
        )}
        <Field label="Name" hint={kind === "schedule" ? "Optional. Without one, the list shows the start of the prompt." : `Optional. Without one, the list says "${WATCH_NAME}".`}>
          <TextInput value={name} onChange={(e) => setName(e.target.value)} maxLength={80} placeholder={kind === "schedule" ? "Morning review" : "Frontend tickets"} />
        </Field>
        <Row gap="l" wrap align="start">
          <Field label="Project">
            <Select
              label="Project"
              value={projectId}
              onChange={setProjectId}
              options={projects.map((p) => ({ value: p.id, label: p.name }))}
            />
          </Field>
          <Field label="Agent">
            <Chips label="Agent" value={agent} onChange={setAgent} options={(Object.keys(AGENTS) as Agent[]).map((value) => ({ value, label: AGENTS[value].label }))} />
          </Field>
        </Row>
        {kind === "schedule" ? <ScheduleFields fields={scheduleFields} /> : <WatchFields watch={watch} fields={watchFields} />}
        {projects.length === 0 ? <Notice tone="warn">Add a project in Wings first. An entry runs inside one.</Notice> : null}
        {error ? <Notice tone="danger">{error}</Notice> : null}
        <Row gap="m" wrap>
          <Button type="submit" onClick={submit} busy={busy} disabled={projects.length === 0}>
            {schedule || watch ? "Save" : "Add entry"}
          </Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </Row>
      </Stack>
    </Card>
  );
}
