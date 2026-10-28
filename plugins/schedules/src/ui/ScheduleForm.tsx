// The form that adds or changes a schedule, or an entry that waits for something to be assigned to you.

import { useState, type FormEvent } from "react";
import { Button, Card, Chip, Chips, Field, FieldGroup, Notice, Row, Select, Stack, TextInput, Textarea } from "wings-plugin-ui";
import { DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, stateOptions, type WatchDraft } from "../lib/assigned";
import type { WatchView } from "../lib/messages";
import { AGENTS, DAYS, DAY_NUMBERS, EVERY_HOURS, type Agent, type Draft, type Schedule } from "../lib/schedule";
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
  onSaveWatch: (draft: WatchDraft) => void;
  onCancel: () => void;
}

const WEEKDAYS = [1, 2, 3, 4, 5];

export function ScheduleForm({ schedule, watch, projects, here, busy, error, onSave, onSaveWatch, onCancel }: Props) {
  // An entry keeps its kind: only a new one picks it.
  const [kind, setKind] = useState<"schedule" | "assigned">(watch ? "assigned" : "schedule");
  const [name, setName] = useState(schedule?.name ?? "");
  const [projectId, setProjectId] = useState(schedule?.projectId ?? watch?.projectId ?? here ?? projects[0]?.id ?? "");
  const [agent, setAgent] = useState<Agent>(schedule?.agent ?? watch?.agent ?? "claude");
  const [prompt, setPrompt] = useState(schedule?.prompt ?? "");
  const [template, setTemplate] = useState(watch?.prompt ?? DEFAULT_PROMPT);
  const [minFree, setMinFree] = useState(String(watch?.minFreeGb ?? DEFAULT_MIN_FREE_GB));
  const [states, setStates] = useState<string[] | undefined>(watch?.states);
  const [mode, setMode] = useState<"every" | "daily">(schedule?.when.kind ?? "daily");
  const [hours, setHours] = useState(schedule?.when.kind === "every" ? schedule.when.hours : 24);
  const [time, setTime] = useState(schedule?.when.kind === "daily" ? schedule.when.time : "09:00");
  const [days, setDays] = useState(schedule?.when.kind === "daily" ? schedule.when.days : WEEKDAYS);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    // Another project has other states, so it picks its own at its first check.
    if (kind === "assigned") return onSaveWatch({ projectId, agent, prompt: template, minFreeGb: Number(minFree), states: watch && watch.projectId !== projectId ? undefined : states });
    onSave({ name, projectId, agent, prompt, when: mode === "every" ? { kind: "every", hours } : { kind: "daily", time, days } });
  };
  const toggleDay = (n: number) => setDays(days.includes(n) ? days.filter((d) => d !== n) : [...days, n]);
  const toggleState = (name: string) => setStates((states ?? []).includes(name) ? (states ?? []).filter((x) => x !== name) : [...(states ?? []), name]);
  const stateNames = watch ? stateOptions({ states, tickets: watch.tickets }) : [];

  return (
    <Card as="form" className="sc-form" onSubmit={submit} aria-label={schedule || watch ? "Edit entry" : "New entry"}>
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
        {kind === "schedule" ? (
          <Field label="Name" hint="Optional. Without one, the list shows the start of the prompt.">
            <TextInput value={name} onChange={(e) => setName(e.target.value)} maxLength={80} placeholder="Morning review" />
          </Field>
        ) : null}
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
        {kind === "schedule" ? (
          <>
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
          </>
        ) : (
          <>
            <Field label="Prompt" hint="Sent to the agent as one line. {id}, {title} and {url} become the ticket's number, title and link.">
              <Textarea value={template} onChange={(e) => setTemplate(e.target.value)} rows={4} />
            </Field>
            <Field label="Keep free" hint="In GB. Wings won't make the worktree when the disk it goes on has less room than this.">
              <TextInput type="number" min={0} step={1} value={minFree} onChange={(e) => setMinFree(e.target.value)} required />
            </Field>
            <FieldGroup
              label="Ask about tickets in these states"
              hint={
                stateNames.length
                  ? "Tickets in other states are listed but not asked about. A ticket is asked about once, and one that moves into these states later is asked about then. Leave all off to ask about every state."
                  : "The states show up after the first check. Wings then picks the ones that mean nobody has started: Open on GitHub and GitLab, and New, To Do, Ready for Development and the like on Azure DevOps."
              }
            >
              {stateNames.length ? (
                <Row gap="s" wrap>
                  {stateNames.map((name) => (
                    <Chip key={name} pressed={(states ?? []).includes(name)} onClick={() => toggleState(name)}>
                      {name}
                    </Chip>
                  ))}
                </Row>
              ) : null}
            </FieldGroup>
            <Notice>Wings asks the project's Azure DevOps, GitHub or GitLab what is assigned to you every 5 minutes, using its origin remote. You are asked first: nothing starts until you press Start.</Notice>
          </>
        )}
        {projects.length === 0 ? <Notice tone="warn">Add a project in Wings first. An entry runs inside one.</Notice> : null}
        {error ? <Notice tone="danger">{error}</Notice> : null}
        <Row gap="m" wrap>
          <Button type="submit" busy={busy} disabled={projects.length === 0}>
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
