// A check's run inside the Checks tab: its jobs and steps on the left, the picked step's log on the right, coloured and
// folded the way the CI marked it. It asks for the run when it opens and when the check's state changes, and for a
// step's log when the step is picked.

import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { Button, List, ListButton, ListItem, ListDetail, Loading, Text } from "wings-plugin-ui";
import { durationText } from "../lib/checklist";
import { parseLog, pickStep, type LogEntry, type LogLine } from "../lib/runlog";
import type { CheckItem, CheckState, Run, RunStep } from "../lib/types";

const STATE_WORD: Record<CheckState, string> = { failed: "Failed", running: "Running", queued: "Queued", passed: "Passed", skipped: "Not run" };
const STATE_MARK: Record<CheckState, string> = { failed: "✕", running: "●", queued: "○", passed: "✓", skipped: "–" };
/** A long log draws its end, where the error is, and the rest on request. */
const SHOWN_LINES = 2_000;

export interface RunViewProps {
  item: CheckItem;
  /** A new object each time the PR's status is read again, which asks for the run again while it is going. */
  version: unknown;
  onLoadRun: () => Promise<Run>;
  onLoadLog: (step: RunStep) => Promise<string[]>;
  onBack: () => void;
}

type Load<T> = { state: "loading" } | { state: "error"; message: string } | { state: "done"; value: T };

function useLoad<T>(load: (() => Promise<T>) | null, deps: unknown[]): [Load<T> | null, () => void] {
  const [value, setValue] = useState<Load<T> | null>(null);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    if (!load) return;
    let live = true;
    // A reload keeps what is drawn until the new answer comes, so a running log doesn't blink.
    setValue((v) => (v?.state === "done" ? v : { state: "loading" }));
    load().then(
      (v) => live && setValue({ state: "done", value: v }),
      (e: Error) => live && setValue({ state: "error", message: String(e?.message ?? e) }),
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);
  return [value, () => setNonce((n) => n + 1)];
}

export function RunView({ item, version, onLoadRun, onLoadLog, onBack }: RunViewProps) {
  const going = item.state === "running" || item.state === "queued";
  const [run, reloadRun] = useLoad(onLoadRun, [item.id, item.state, going ? version : null]);
  const [picked, setPicked] = useState<string | null>(null);
  const steps = useMemo(() => (run?.state === "done" ? run.value.jobs.flatMap((j) => j.steps) : []), [run]);
  const step = steps.find((s) => s.id === picked) ?? pickStep(steps.filter((s) => s.log));
  const [log, reloadLog] = useLoad(step?.log ? () => onLoadLog(step) : null, [step?.id, step?.state, step?.seconds]);
  const runUrl = run?.state === "done" ? run.value.url : null;
  const url = (runUrl ?? item.url)?.startsWith("https://") ? (runUrl ?? item.url) : null;

  return (
    <div className="pr-run" data-testid="pr-run-view">
      <div className="pr-run-head">
        <Button variant="ghost" size="sm" onClick={onBack}>← Checks</Button>
        <span className="pr-check-mark" data-state={item.state} role="img" aria-label={STATE_WORD[item.state]}>{STATE_MARK[item.state]}</span>
        <strong className="pr-run-name">{item.name}</strong>
        {durationText(item.seconds) ? <Text tone="muted" size="small" as="span">{durationText(item.seconds)}</Text> : null}
        <span className="pr-run-actions">
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
          {url ? <Button variant="ghost" size="sm" onClick={() => void wings.openUrl(url)}>Open</Button> : null}
        </span>
      </div>
      {!run || run.state === "loading" ? <Loading>Loading the run…</Loading> : null}
      {run?.state === "error" ? <Text tone="danger" size="small" role="alert">{run.message}</Text> : null}
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
    </div>
  );
}

function Steps({ run, current, onPick }: { run: Run; current: string | null; onPick: (id: string) => void }) {
  let stage: string | null = null;
  return (
    <div className="pr-run-jobs">
      {run.jobs.map((j) => {
        const heading = j.stage && j.stage !== stage ? j.stage : null;
        stage = j.stage;
        return (
          <Fragment key={j.id}>
            {heading ? <Text tone="muted" size="tiny" className="pr-run-stage">{heading}</Text> : null}
            <section className="pr-run-job" aria-label={j.name}>
              <div className="pr-run-job-head">
                <span className="pr-check-mark" data-state={j.state} role="img" aria-label={STATE_WORD[j.state]}>{STATE_MARK[j.state]}</span>
                <strong>{j.name}</strong>
                <Text tone="muted" size="tiny" as="span" className="pr-run-took">{durationText(j.seconds)}</Text>
              </div>
              {j.steps.length ? (
                <List compact>
                  {j.steps.map((s) => (
                    <ListItem key={s.id}>
                      <ListButton className="pr-run-step" aria-current={s.id === current ? "true" : undefined} disabled={!s.log} onClick={() => onPick(s.id)}>
                        <span className="pr-check-mark" data-state={s.state} role="img" aria-label={STATE_WORD[s.state]}>{STATE_MARK[s.state]}</span>
                        <span className="pr-run-step-name">{s.name}</span>
                        <Text tone="muted" size="tiny" as="span" className="pr-run-took">{durationText(s.seconds)}</Text>
                      </ListButton>
                    </ListItem>
                  ))}
                </List>
              ) : (
                <Text tone="muted" size="tiny" className="pr-run-none">{j.state === "queued" ? "Waiting to start" : "No steps"}</Text>
              )}
            </section>
          </Fragment>
        );
      })}
    </div>
  );
}

function StepLog({ step, log }: { step: RunStep; log: Load<string[]> | null }) {
  const [all, setAll] = useState(false);
  const entries = useMemo(() => (log?.state === "done" ? parseLog(log.value) : []), [log]);
  const total = entries.reduce((n, e) => n + ("title" in e ? e.lines.length + 1 : 1), 0);
  const shown = useMemo(() => (all ? entries : lastLines(entries, SHOWN_LINES)), [entries, all]);
  const box = useRef<HTMLPreElement>(null);
  // A step that failed or is still going opens at its end, where the error or the newest line is.
  const atEnd = step.state === "failed" || step.state === "running";
  useEffect(() => {
    if (atEnd && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [shown, atEnd]);

  if (!log || log.state === "loading") return <Loading>Loading the log…</Loading>;
  if (log.state === "error") return <Text tone="danger" size="small" role="alert">{log.message}</Text>;
  if (!entries.length) return <Text tone="muted" size="small">{step.state === "running" ? "No output yet." : "This step wrote nothing."}</Text>;
  return (
    <div className="pr-run-log">
      {shown !== entries ? (
        <Text tone="muted" size="tiny" as="div" className="pr-run-more">
          Showing the last {SHOWN_LINES} of {total} lines.{" "}
          <Button variant="link" size="xs" onClick={() => setAll(true)}>Show all</Button>
        </Text>
      ) : null}
      <pre ref={box} className="pr-log pr-run-pre" tabIndex={0} aria-label={`Log of ${step.name}`}>
        {shown.map((e) => ("title" in e ? <Group key={e.title.n} entry={e} /> : <Line key={e.n} line={e} />))}
      </pre>
    </div>
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

function Group({ entry }: { entry: { title: LogLine; lines: LogLine[] } }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className="pr-run-line pr-run-group" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <span className="pr-run-n">{entry.title.n}</span>
        <span className="pr-run-text">
          <span aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</span> <Spans line={entry.title} />
        </span>
      </button>
      {open ? entry.lines.map((l) => <Line key={l.n} line={l} />) : null}
    </>
  );
}

function Line({ line }: { line: LogLine }) {
  return (
    <span className="pr-run-line" data-tone={line.tone ?? undefined}>
      <span className="pr-run-n" title={line.time ?? undefined}>{line.n}</span>
      <span className="pr-run-text">
        <Spans line={line} />
      </span>
    </span>
  );
}

function Spans({ line }: { line: LogLine }) {
  return (
    <>
      {line.spans.map((s, i) =>
        s.fg === undefined && !s.bold && !s.dim && !s.italic && !s.underline ? (
          <Fragment key={i}>{s.text}</Fragment>
        ) : (
          <span key={i} data-fg={s.fg} data-bold={s.bold ? "" : undefined} data-dim={s.dim ? "" : undefined} data-italic={s.italic ? "" : undefined} data-underline={s.underline ? "" : undefined}>
            {s.text}
          </span>
        ),
      )}
    </>
  );
}
