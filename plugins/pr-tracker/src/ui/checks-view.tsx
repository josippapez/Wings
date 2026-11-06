// The Checks tab: every check or policy of the pull request, grouped by how it stands. It draws what it's given; a
// failed check's log is asked for (`onLoadFailure`) only when the row is opened, and shown as plain text.

import { useMemo, useRef, useState } from "react";
import { ago, Badge, Button, CodeBlock, ListButton, Notice, Row, Stack, Text, useAction } from "wings-plugin-ui";
import { groupChecks, durationText, hasLog, logTail, withUnresolved, type Group, type GroupId } from "../lib/checklist";
import type { CheckItem, CheckState, FailureDetail, Failing, Run, RunRef, RunStep, Status } from "../lib/types";
import { RunView } from "./run-view";

const STATE_WORD: Record<CheckState, string> = { failed: "Failed", running: "Running", queued: "Queued", passed: "Passed", skipped: "Not run" };
const STATE_MARK: Record<CheckState, string> = { failed: "✕", running: "●", queued: "○", passed: "✓", skipped: "–" };
const STATE_TONE = { failed: "danger", running: "warn", queued: "muted", passed: "success", skipped: "muted" } as const;

interface ChecksViewProps {
  status: Status;
  /** Open comment threads, once they have loaded; a comment requirement says how many. */
  unresolved: number | null;
  onLoadFailure?: (f: Failing) => Promise<FailureDetail[]>;
  /** A check's run, for a check whose CI has jobs and steps to show. */
  onLoadRun?: (run: RunRef) => Promise<Run>;
  onLoadRunLog?: (run: RunRef, step: RunStep) => Promise<string[]>;
}

type Load = { state: "loading" } | { state: "error"; message: string } | { state: "done"; details: FailureDetail[] };

export function ChecksView({ status, unresolved, onLoadFailure, onLoadRun, onLoadRunLog }: ChecksViewProps) {
  const items = useMemo(() => withUnresolved(status.items ?? [], unresolved), [status.items, unresolved]);
  const groups = useMemo(() => groupChecks(items), [items]);
  const [open, setOpen] = useState<Partial<Record<GroupId, boolean>>>({});
  const [loads, setLoads] = useState<Record<string, Load>>({});
  const asked = useRef(new Set<string>());
  const [opened, setOpened] = useState<string | null>(null);

  if (!items.length) {
    const { passed, failed, running } = status.checks;
    return <Text tone="muted">{passed + failed + running ? "This provider didn't list the checks by name." : "No checks reported."}</Text>;
  }

  const run = items.find((c) => c.id === opened && c.run);
  if (run?.run && onLoadRun && onLoadRunLog) {
    const ref = run.run;
    return <RunView key={run.id} item={run} version={status} onLoadRun={() => onLoadRun(ref)} onLoadLog={(step) => onLoadRunLog(ref, step)} onBack={() => setOpened(null)} />;
  }

  // The log belongs to the commit it ran for, so a new push asks again.
  const loadKey = (c: CheckItem) => `${status.key}:${c.id}`;
  const ask = (c: CheckItem) => {
    const key = loadKey(c);
    if (asked.current.has(key) || !onLoadFailure || !c.failing) return;
    asked.current.add(key);
    setLoads((l) => ({ ...l, [key]: { state: "loading" } }));
    onLoadFailure(c.failing).then(
      (details) => setLoads((l) => ({ ...l, [key]: { state: "done", details } })),
      (e: Error) => {
        asked.current.delete(key);
        setLoads((l) => ({ ...l, [key]: { state: "error", message: String(e.message ?? e) } }));
      },
    );
  };

  return (
    <Stack gap="m" className="pr-checks" data-testid="pr-checks-view">
      {groups.map((g) => (
        <GroupSection key={g.id} group={g} open={open[g.id] ?? (g.id === "failed" || g.id === "active")} onToggle={() => setOpen((o) => ({ ...o, [g.id]: !(o[g.id] ?? (g.id === "failed" || g.id === "active")) }))} loadKey={loadKey} loads={loads} onAsk={ask} canLoad={Boolean(onLoadFailure)} onOpenRun={onLoadRun && onLoadRunLog ? setOpened : undefined} />
      ))}
    </Stack>
  );
}

interface SectionProps {
  group: Group;
  open: boolean;
  onToggle: () => void;
  loadKey: (c: CheckItem) => string;
  loads: Record<string, Load>;
  onAsk: (c: CheckItem) => void;
  canLoad: boolean;
  onOpenRun?: (id: string) => void;
}

function GroupSection({ group, open, onToggle, loadKey, loads, onAsk, canLoad, onOpenRun }: SectionProps) {
  const listId = `pr-checks-${group.id}`;
  return (
    <Stack as="section" gap="s" className="pr-check-group" data-group={group.id} aria-label={group.label}>
      <ListButton className="pr-check-group-head" aria-expanded={open} aria-controls={open ? listId : undefined} onClick={onToggle} data-testid={`pr-checks-${group.id}-toggle`}>
        {WINGS_NATIVE ? null : <Text aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</Text>}
        <Text weight="strong">{group.label}</Text>
        <Text tone="muted" className="pr-check-count">{group.items.length}</Text>
      </ListButton>
      {open ? (
        <Stack as="ul" id={listId} gap="xs" className="pr-check-list" aria-label={`${group.label} checks`}>
          {group.items.map((c) => (
            <CheckRow key={c.id} item={c} load={loads[loadKey(c)]} onAsk={onAsk} canLoad={canLoad} onOpenRun={c.run ? onOpenRun : undefined} />
          ))}
        </Stack>
      ) : null}
    </Stack>
  );
}

function CheckRow({ item: c, load, onAsk, canLoad, onOpenRun }: { item: CheckItem; load: Load | undefined; onAsk: (c: CheckItem) => void; canLoad: boolean; onOpenRun?: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const opening = useAction();
  const expandable = !onOpenRun && canLoad && c.state === "failed" && hasLog(c.failing);
  const url = c.url?.startsWith("https://") ? c.url : null;
  const time = c.at ? ago(c.at) : "";
  const took = durationText(c.seconds);
  const detailId = `pr-check-detail-${c.id.replace(/[^\w-]/g, "_")}`;
  const main = (
    <>
      <Text tone={STATE_TONE[c.state]} className="pr-check-mark" data-state={c.state} role="img" aria-label={STATE_WORD[c.state]}>{STATE_MARK[c.state]}</Text>
      <Stack gap="xs" grow className="pr-check-text">
        <Row gap="s" wrap className="pr-check-line">
          <Text weight="medium" className="pr-check-name">{c.name}</Text>
          {c.required === true ? <Badge size="sm" variant="outline" className="pr-tag" data-tone="required">Required</Badge> : c.required === false ? <Badge size="sm" variant="outline" className="pr-tag">Optional</Badge> : null}
          {c.kind && c.kind !== c.name ? <Text tone="muted" size="tiny">{c.kind}</Text> : null}
        </Row>
        {c.note ? <Text tone={c.state === "failed" ? "danger" : "muted"} size="small" className="pr-check-note">{c.note}</Text> : null}
      </Stack>
      {expandable && !WINGS_NATIVE ? <Text aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</Text> : null}
      {onOpenRun ? <Text aria-hidden="true" className="pr-check-go">›</Text> : null}
    </>
  );
  return (
    <Stack as="li" gap="xs" className="pr-check" data-state={c.state} data-testid="pr-check">
      <Row gap="s" className="pr-check-row">
        {onOpenRun ? (
          <ListButton className="pr-check-main" aria-label={`${c.name}: see its jobs, steps and logs`} onClick={() => onOpenRun(c.id)}>
            {main}
          </ListButton>
        ) : expandable ? (
          <ListButton
            className="pr-check-main"
            aria-expanded={open}
            aria-controls={open ? detailId : undefined}
            onClick={() => {
              setOpen((o) => !o);
              if (!open) onAsk(c);
            }}
          >
            {main}
          </ListButton>
        ) : (
          <Row gap="s" grow className="pr-check-main">{main}</Row>
        )}
        {time || took ? (
          <Text tone="muted" size="small" className="pr-check-time" title={c.at ? new Date(c.at).toLocaleString() : undefined}>
            {[time, took].filter(Boolean).join(" · ")}
          </Text>
        ) : null}
        <Row className="pr-check-open">{url ? <Button size="sm" variant="ghost" aria-label={`Open ${c.name}`} onClick={() => void opening.run(() => wings.openUrl(url))}>Open</Button> : null}</Row>
      </Row>
      {opening.error ? <Notice tone="danger">{opening.error}</Notice> : null}
      {open ? <FailureBody id={detailId} load={load} /> : null}
    </Stack>
  );
}

function FailureBody({ id, load }: { id: string; load: Load | undefined }) {
  return (
    <Stack gap="s" id={id} className="pr-check-detail" aria-live="polite">
      {!load || load.state === "loading" ? <Text tone="muted" size="small">Loading the log…</Text> : null}
      {load?.state === "error" ? <Notice tone="danger">{load.message}</Notice> : null}
      {load?.state === "done" ? load.details.map((d, i) => <Detail key={i} d={d} />) : null}
    </Stack>
  );
}

function Detail({ d }: { d: FailureDetail }) {
  const tail = d.log ? logTail(d.log) : null;
  return (
    <Stack gap="xs" className="pr-fail">
      <Text size="small"><Text weight="strong">{d.step ?? d.check}</Text>{d.summary ? `: ${d.summary}` : ""}</Text>
      {tail ? (
        <>
          {tail.hidden ? <Text tone="muted" size="tiny">Showing the last {tail.text.split("\n").length} of {tail.text.split("\n").length + tail.hidden} lines.</Text> : null}
          <CodeBlock className="pr-log" code={tail.text} label={`Log of ${d.step ?? d.check ?? "the check"}`} />
        </>
      ) : (
        <Text tone="muted" size="tiny">No log for this step.</Text>
      )}
    </Stack>
  );
}
