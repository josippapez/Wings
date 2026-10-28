// The form that adds or changes a schedule.

import { useState, type FormEvent } from "react";
import { Button, Card, Chip, Chips, Field, FieldGroup, Notice, Row, Select, Stack, TextInput, Textarea } from "wings-plugin-ui";
import { AGENTS, DAYS, DAY_NUMBERS, EVERY_HOURS, type Agent, type Draft, type Schedule } from "../lib/schedule";
import "./schedules.css";

interface Props {
  /** The schedule being changed, or null for a new one. */
  schedule: Schedule | null;
  projects: { id: string; name: string }[];
  busy: boolean;
  /** What the last save failed with. */
  error: string | null;
  onSave: (draft: Draft) => void;
  onCancel: () => void;
}

const WEEKDAYS = [1, 2, 3, 4, 5];

export function ScheduleForm({ schedule, projects, busy, error, onSave, onCancel }: Props) {
  const [name, setName] = useState(schedule?.name ?? "");
  const [projectId, setProjectId] = useState(schedule?.projectId ?? projects[0]?.id ?? "");
  const [agent, setAgent] = useState<Agent>(schedule?.agent ?? "claude");
  const [prompt, setPrompt] = useState(schedule?.prompt ?? "");
  const [mode, setMode] = useState<"every" | "daily">(schedule?.when.kind ?? "daily");
  const [hours, setHours] = useState(schedule?.when.kind === "every" ? schedule.when.hours : 24);
  const [time, setTime] = useState(schedule?.when.kind === "daily" ? schedule.when.time : "09:00");
  const [days, setDays] = useState(schedule?.when.kind === "daily" ? schedule.when.days : WEEKDAYS);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    onSave({ name, projectId, agent, prompt, when: mode === "every" ? { kind: "every", hours } : { kind: "daily", time, days } });
  };
  const toggleDay = (n: number) => setDays(days.includes(n) ? days.filter((d) => d !== n) : [...days, n]);

  return (
    <Card as="form" className="sc-form" onSubmit={submit} aria-label={schedule ? "Edit schedule" : "New schedule"}>
      <Stack gap="l">
        <Field label="Name" hint="Optional. Without one, the list shows the start of the prompt.">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} maxLength={80} placeholder="Morning review" />
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
        <Field label="Prompt" hint="What the agent is asked to do. It goes in as one line: line breaks become spaces.">
          <Textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={4} placeholder="Check yesterday's failed CI runs and open a fix for the flaky test." />
        </Field>
        <FieldGroup label="When">
          <Stack gap="m">
            <Chips
              label="When"
              value={mode}
              onChange={setMode}
              options={[
                { value: "daily", label: "At a time of day" },
                { value: "every", label: "Every few hours" },
              ]}
            />
            {mode === "every" ? (
              <Field label="Every">
                <Select
                  label="Every"
                  value={String(hours)}
                  onChange={(v) => setHours(Number(v))}
                  options={EVERY_HOURS.map((h) => ({ value: String(h), label: h === 1 ? "1 hour" : `${h} hours` }))}
                />
              </Field>
            ) : (
              <Row gap="l" wrap align="start">
                <Field label="Time">
                  <TextInput type="time" value={time} onChange={(e) => setTime(e.target.value)} required />
                </Field>
                <FieldGroup label="Days">
                  <Row gap="s" wrap>
                    {DAYS.map((label, i) => (
                      <Chip key={label} pressed={days.includes(DAY_NUMBERS[i])} onClick={() => toggleDay(DAY_NUMBERS[i])}>
                        {label}
                      </Chip>
                    ))}
                  </Row>
                </FieldGroup>
              </Row>
            )}
          </Stack>
        </FieldGroup>
        {projects.length === 0 ? <Notice tone="warn">Add a project in Wings first. A schedule runs inside one.</Notice> : null}
        {error ? <Notice tone="danger">{error}</Notice> : null}
        <Row gap="m" wrap>
          <Button type="submit" busy={busy} disabled={projects.length === 0}>
            {schedule ? "Save" : "Add schedule"}
          </Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </Row>
      </Stack>
    </Card>
  );
}
