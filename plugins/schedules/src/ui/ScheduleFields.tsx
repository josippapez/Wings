// The fields only a schedule has: its prompt and when it runs.

import { useState } from "react";
import { Chip, Chips, Field, FieldGroup, Row, Select, Stack, TextInput, Textarea } from "wings-plugin-ui";
import { DAYS, DAY_NUMBERS, EVERY_HOURS, type Draft, type Schedule } from "../lib/schedule";

const WEEKDAYS = [1, 2, 3, 4, 5];

/** What the schedule fields hold, from `schedule` or a new one's defaults, and the part of its draft they make. */
export function useScheduleFields(schedule: Schedule | null) {
  const [prompt, setPrompt] = useState(schedule?.prompt ?? "");
  const [mode, setMode] = useState<"every" | "daily">(schedule?.when.kind ?? "daily");
  const [hours, setHours] = useState(schedule?.when.kind === "every" ? schedule.when.hours : 24);
  const [time, setTime] = useState(schedule?.when.kind === "daily" ? schedule.when.time : "09:00");
  const [days, setDays] = useState(schedule?.when.kind === "daily" ? schedule.when.days : WEEKDAYS);
  const draft = (): Pick<Draft, "prompt" | "when"> => ({ prompt, when: mode === "every" ? { kind: "every", hours } : { kind: "daily", time, days } });
  return { prompt, setPrompt, mode, setMode, hours, setHours, time, setTime, days, setDays, draft };
}

export function ScheduleFields({ fields }: { fields: ReturnType<typeof useScheduleFields> }) {
  const { prompt, setPrompt, mode, setMode, hours, setHours, time, setTime, days, setDays } = fields;
  const toggleDay = (n: number) => setDays(days.includes(n) ? days.filter((d) => d !== n) : [...days, n]);
  return (
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
  );
}
