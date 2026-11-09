// A check's run inside the Checks tab: its jobs and steps on the left, the picked step's log on the right, coloured and
// folded the way the CI marked it. It asks for the run when it opens and when the check's state changes, and for a
// step's log when the step is picked.

import { Fragment, useEffect, useMemo, useState } from "react";
import { Button, List, ListButton, ListDetail, ListItem, Loading, Notice, Row, Stack, Text, useAction } from "wings-plugin-ui";
import { durationText } from "../lib/checklist";
import { parseLog, pickStep, type LogEntry, type LogLine, type Span, type Tone } from "../lib/runlog";
import type { CheckItem, Run, RunStep } from "../lib/types";
import { StateMark } from "./parts";

/** A long log draws its end, where the error is, and the rest on request. The native app draws a page of at most 5,000 nodes, so it shows fewer lines and has no rest. */
const SHOWN_LINES = WINGS_NATIVE ? 500 : 2_000;

/** What the native app does with the CI's markup and ANSI colours: it has no CSS to style them with. */
const LINE_TONE: Record<NonNullable<Tone>, "danger" | "warn" | "info" | "muted"> = { error: "danger", warning: "warn", command: "info", section: "info", debug: "muted" };
const FG_TONE: Record<number, "danger" | "success" | "warn" | "info" | "muted"> = { 0: "muted", 8: "muted", 1: "danger", 9: "danger", 2: "success", 10: "success", 3: "warn", 11: "warn", 4: "info", 12: "info", 5: "info", 13: "info", 6: "info", 14: "info" };

export interface RunViewProps {
  item: CheckItem;
  /** A new object each time the PR's status is read again, which asks for the run again while it is going. */
  version: unknown;
  onLoadRun: () => Promise<Run>;
  onLoadLog: (step: RunStep) => Promise<string[]>;
  onBack: () => void;
}

type Load<T> = { state: "loading" } | { state: "error"; message: string } | { state: "done"; value: T };

const LOADING = { state: "loading" } as const;

/** Loads what `id` names, and again when `deps` change. */
function useLoad<T>(load: (() => Promise<T>) | null, id: unknown, deps: unknown[]): [Load<T> | null, () => void] {
  const [value, setValue] = useState<{ id: unknown; load: Load<T> } | null>(null);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    if (!load) return;
    let live = true;
    // A reload keeps what is drawn until the new answer comes, so a running log doesn't blink.
    setValue((v) => (v !== null && v.id === id && v.load.state === "done" ? v : { id, load: LOADING }));
    load().then(
      (v) => live && setValue({ id, load: { state: "done", value: v } }),
      (e: Error) => live && setValue({ id, load: { state: "error", message: String(e?.message ?? e) } }),
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, ...deps, nonce]);
  // What was loaded for another id is never shown for this one: a picked step showed the log of the step picked before
  // it until its own came, so the click seemed to do nothing for as long as the CLI took.
  return [value && (value.id === id ? value.load : LOADING), () => setNonce((n) => n + 1)];
}

export function RunView({ item, version, onLoadRun, onLoadLog, onBack }: RunViewProps) {
  const going = item.state === "running" || item.state === "queued";
  const [run, reloadRun] = useLoad(onLoadRun, item.id, [item.state, going ? version : null]);
  const [picked, setPicked] = useState<string | null>(null);
  const opening = useAction();
  const steps = useMemo(() => (run?.state === "done" ? run.value.jobs.flatMap((j) => j.steps) : []), [run]);
  const step = steps.find((s) => s.id === picked) ?? pickStep(steps.filter((s) => s.log));
  const [log, reloadLog] = useLoad(step?.log ? () => onLoadLog(step) : null, step?.id, [step?.state, step?.seconds]);
  const runUrl = run?.state === "done" ? run.value.url : null;
  const url = (runUrl ?? item.url)?.startsWith("https://") ? (runUrl ?? item.url) : null;

  return (
    <Stack gap="m" className="pr-run" data-testid="pr-run-view">
      <Row gap="s" className="pr-run-head">
        <Button variant="ghost" size="sm" onClick={onBack}>← Checks</Button>
        <StateMark state={item.state} />
        <Text weight="strong" truncate className="pr-run-name">{item.name}</Text>
        {durationText(item.seconds) ? <Text tone="muted" size="small">{durationText(item.seconds)}</Text> : null}
        <Row gap="s" grow justify="end" className="pr-run-actions">
          <Button
            variant="ghost"
            size="sm"
            busy={run?.state === "loading" || log?.state === "loading"}
            onClick={() => {
              reloadRun();
              reloadLog();
            }}
          >
            Refresh
          </Button>
          {url ? <Button variant="ghost" size="sm" onClick={() => void opening.run(() => wings.openUrl(url))}>Open</Button> : null}
        </Row>
      </Row>
      {opening.error ? <Notice tone="danger">{opening.error}</Notice> : null}
      {!run || run.state === "loading" ? <Loading>Loading the run…</Loading> : null}
      {run?.state === "error" ? <Notice tone="danger">{run.message}</Notice> : null}
      {run?.state === "done" && !run.value.jobs.length ? <Text tone="muted" size="small">{going ? "The run hasn't started any jobs yet." : "This run has no jobs."}</Text> : null}
      {run?.state === "done" && run.value.note ? <Text tone="muted" size="small">{run.value.note}</Text> : null}
      {run?.state === "done" && run.value.jobs.length ? (
        <ListDetail
          className="pr-run-split"
          listWidth={300}
          listLabel="Jobs and steps"
          detailLabel={step ? `Log of ${step.name}` : "Log"}
          onBack={() => setPicked(null)}
          backLabel="← Steps"
          list={<Steps run={run.value} current={step?.id ?? null} onPick={setPicked} />}
          detail={step ? <StepLog step={step} log={log} /> : undefined}
          empty={<Text tone="muted" size="small">Pick a step to see its log.</Text>}
        />
      ) : null}
    </Stack>
  );
}

function Steps({ run, current, onPick }: { run: Run; current: string | null; onPick: (id: string) => void }) {
  let stage: string | null = null;
  return (
    <Stack gap="m" className="pr-run-jobs">
      {run.jobs.map((j) => {
        const heading = j.stage && j.stage !== stage ? j.stage : null;
        stage = j.stage;
        return (
          <Fragment key={j.id}>
            {heading ? <Text tone="muted" size="tiny" className="pr-run-stage">{heading}</Text> : null}
            <Stack as="section" gap="xs" className="pr-run-job" aria-label={j.name}>
              <Row gap="s" className="pr-run-job-head">
                <StateMark state={j.state} />
                <Text weight="strong" grow>{j.name}</Text>
                <Text tone="muted" size="tiny" className="pr-run-took">{durationText(j.seconds)}</Text>
              </Row>
              {j.steps.length ? (
                <List compact>
                  {j.steps.map((s) => (
                    <ListItem key={s.id}>
                      <ListButton className="pr-run-step" aria-current={s.id === current ? "true" : undefined} disabled={!s.log} onClick={() => onPick(s.id)}>
                        <StateMark state={s.state} />
                        <Text grow truncate weight={s.id === current ? "strong" : undefined} className="pr-run-step-name">{s.name}</Text>
                        <Text tone="muted" size="tiny" className="pr-run-took">{durationText(s.seconds)}</Text>
                      </ListButton>
                    </ListItem>
                  ))}
                </List>
              ) : (
                <Text tone="muted" size="tiny" className="pr-run-none">{j.state === "queued" ? "Waiting to start" : "No steps"}</Text>
              )}
            </Stack>
          </Fragment>
        );
      })}
    </Stack>
  );
}

function StepLog({ step, log }: { step: RunStep; log: Load<string[]> | null }) {
  const [all, setAll] = useState(false);
  const entries = useMemo(() => (log?.state === "done" ? parseLog(log.value) : []), [log]);
  const total = entries.reduce((n, e) => n + ("title" in e ? e.lines.length + 1 : 1), 0);
  const shown = useMemo(() => (all ? entries : lastLines(entries, SHOWN_LINES)), [entries, all]);

  if (!log || log.state === "loading") return <Loading>Loading the log…</Loading>;
  if (log.state === "error") return <Notice tone="danger">{log.message}</Notice>;
  if (!entries.length) return <Text tone="muted" size="small">{step.state === "running" ? "No output yet." : "This step wrote nothing."}</Text>;
  const last = shown[shown.length - 1];
  const digits = String("title" in last ? (last.lines.at(-1) ?? last.title).n : last.n).length;
  return (
    <Stack gap="xs" className="pr-run-log">
      {shown !== entries ? (
        <Row gap="s" className="pr-run-more">
          <Text tone="muted" size="tiny">Showing the last {SHOWN_LINES} of {total} lines.</Text>
          {WINGS_NATIVE ? null : <Button variant="link" size="xs" onClick={() => setAll(true)}>Show all</Button>}
        </Row>
      ) : null}
      <Stack gap="none" className="pr-log pr-run-pre" tabIndex={0} role="region" aria-label={`Log of ${step.name}`}>
        {shown.map((e) => ("title" in e ? <Group key={e.title.n} entry={e} digits={digits} /> : <Line key={e.n} line={e} digits={digits} />))}
      </Stack>
    </Stack>
  );
}

/** The end of a log, `max` lines, keeping a group whole once any of it is in. */
function lastLines(entries: LogEntry[], max: number): LogEntry[] {
  let n = 0;
  let i = entries.length;
  while (i > 0 && n < max) {
    const e = entries[--i];
    n += "title" in e ? e.lines.length + 1 : 1;
  }
  return i === 0 ? entries : entries.slice(i);
}

/**
 * A line's number. pr.css sets the numbers right-aligned in a column, which the app has no CSS for: there each is
 * padded to as many digits as the last line's, and the mono font lines them up, or each line's text started after its
 * own number.
 */
const numberOf = (n: number, digits: number) => (WINGS_NATIVE ? String(n).padStart(digits) : n);

function Group({ entry, digits }: { entry: { title: LogLine; lines: LogLine[] }; digits: number }) {
  const [open, setOpen] = useState(false);
  const toggle = () => setOpen((o) => !o);
  const title = (
    <Text mono size="small" grow weight={WINGS_NATIVE ? "strong" : undefined} className="pr-run-text">
      {WINGS_NATIVE ? null : <Text aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾ </Text>}
      <Spans line={entry.title} />
    </Text>
  );
  return (
    <>
      {WINGS_NATIVE ? (
        // The app draws a list button's chevron before what it holds, which put the number after it, out of the
        // numbers' column: the number stays in the column, and the button holds the title in the text's.
        <Row gap="m" align="baseline">
          <Text mono size="tiny" tone="muted" numeric>{numberOf(entry.title.n, digits)}</Text>
          <ListButton align="baseline" aria-expanded={open} onClick={toggle}>{title}</ListButton>
        </Row>
      ) : (
        <ListButton className="pr-run-line pr-run-group" aria-expanded={open} onClick={toggle}>
          <Text mono size="tiny" tone="muted" numeric className="pr-run-n">{entry.title.n}</Text>
          {title}
        </ListButton>
      )}
      {open ? entry.lines.map((l) => <Line key={l.n} line={l} digits={digits} />) : null}
    </>
  );
}

function Line({ line, digits }: { line: LogLine; digits: number }) {
  return (
    <Row gap="m" align="baseline" className="pr-run-line" data-tone={line.tone ?? undefined}>
      <Text mono size="tiny" tone="muted" numeric className="pr-run-n" title={line.time ?? undefined}>{numberOf(line.n, digits)}</Text>
      <Text mono size="small" grow tone={WINGS_NATIVE && line.tone ? LINE_TONE[line.tone] : undefined} className="pr-run-text">
        <Spans line={line} />
      </Text>
    </Row>
  );
}

const spanTone = (s: Span) => (WINGS_NATIVE ? (s.dim ? "muted" : s.fg === undefined ? undefined : FG_TONE[s.fg]) : undefined);

function Spans({ line }: { line: LogLine }) {
  return (
    <>
      {line.spans.map((s, i) =>
        s.fg === undefined && !s.bold && !s.dim && !s.italic && !s.underline ? (
          <Fragment key={i}>{s.text}</Fragment>
        ) : (
          <Text key={i} tone={spanTone(s)} weight={WINGS_NATIVE && s.bold ? "strong" : undefined} data-fg={s.fg} data-bold={s.bold ? "" : undefined} data-dim={s.dim ? "" : undefined} data-italic={s.italic ? "" : undefined} data-underline={s.underline ? "" : undefined}>
            {s.text}
          </Text>
        ),
      )}
    </>
  );
}
