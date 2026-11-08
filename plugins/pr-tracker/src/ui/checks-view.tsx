// The Checks tab: every check or policy of the pull request, grouped by how it stands. It draws what it's given; a
// failed check's log is asked for (`onLoadFailure`) only when the row is opened, and shown as plain text.

import { useMemo, useRef, useState } from "react";
import { ago, Badge, Bar, Button, CodeBlock, ListButton, Notice, Row, Stack, Svg, Text, useAction } from "wings-plugin-ui";
import { groupChecks, durationText, hasLog, logTail, withUnresolved, type Group, type GroupId } from "../lib/checklist";
import type { CheckItem, FailureDetail, Failing, Run, RunRef, RunStep, Status } from "../lib/types";
import { Column, StateMark, Tag } from "./parts";
import { RunView } from "./run-view";

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
        {/* pr.css's count is a muted pill. */}
        {WINGS_NATIVE ? (
          <Badge variant="secondary" size="sm"><Text tone="muted">{group.items.length}</Text></Badge>
        ) : (
          <Text tone="muted" className="pr-check-count">{group.items.length}</Text>
        )}
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
  const at = c.at ? ago(c.at) : "";
  const took = durationText(c.seconds);
  const detailId = `pr-check-detail-${c.id.replace(/[^\w-]/g, "_")}`;
  const main = (
    <>
      <StateMark state={c.state} />
      <Stack gap="xs" grow className="pr-check-text">
        <Row gap="s" wrap className="pr-check-line">
          <Text weight="medium" className="pr-check-name">{c.name}</Text>
          {c.required === true ? <Tag required>Required</Tag> : c.required === false ? <Tag>Optional</Tag> : null}
          {c.kind && c.kind !== c.name ? <Text tone="muted" size="tiny">{c.kind}</Text> : null}
        </Row>
        {c.note ? <Text tone={c.state === "failed" ? "danger" : "muted"} size="small" className="pr-check-note">{c.note}</Text> : null}
      </Stack>
      {expandable && !WINGS_NATIVE ? <Text aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</Text> : null}
      {onOpenRun ? <Go /> : null}
    </>
  );
  const time = at || took ? (
    <Text tone="muted" size="small" className="pr-check-time" title={c.at ? new Date(c.at).toLocaleString() : undefined}>
      {[at, took].filter(Boolean).join(" · ")}
    </Text>
  ) : null;
  const go = url ? <Button size="sm" variant="ghost" aria-label={`Open ${c.name}`} onClick={() => void opening.run(() => wings.openUrl(url))}>Open</Button> : null;
  const toggle = () => {
    setOpen((o) => !o);
    if (!open) onAsk(c);
  };
  if (WINGS_NATIVE) {
    // pr.css's row: a hairline over it, the mark, name and note at the top of their lines, then the time, then a slot
    // as wide as an Open button so every row's time ends in one place. A row that's no button is padded as a button's
    // 6px and edge are, so its mark sits under the others.
    const line = <Row gap="m" align="start" grow>{main}</Row>;
    return (
      <Bar edge="top" gap="xs">
        <Row gap="m">
          {onOpenRun ? (
            <ListButton aria-label={`${c.name}: see its jobs, steps and logs`} onClick={() => onOpenRun(c.id)}>{line}</ListButton>
          ) : expandable ? (
            <ListButton aria-expanded={open} onClick={toggle}>{line}</ListButton>
          ) : (
            <Row gap="none" grow><Column width={7} />{line}<Column width={7} /></Row>
          )}
          {time}
          <Column width={52}><Row justify="end">{go}</Row></Column>
        </Row>
        {opening.error ? <Notice tone="danger">{opening.error}</Notice> : null}
        {open ? <FailureBody id={detailId} load={load} /> : null}
      </Bar>
    );
  }
  return (
    <Stack as="li" gap="xs" className="pr-check" data-state={c.state} data-testid="pr-check">
      <Row gap="s" className="pr-check-row">
        {onOpenRun ? (
          <ListButton className="pr-check-main" aria-label={`${c.name}: see its jobs, steps and logs`} onClick={() => onOpenRun(c.id)}>
            {main}
          </ListButton>
        ) : expandable ? (
          <ListButton className="pr-check-main" aria-expanded={open} aria-controls={open ? detailId : undefined} onClick={toggle}>
            {main}
          </ListButton>
        ) : (
          <Row gap="s" grow className="pr-check-main">{main}</Row>
        )}
        {time}
        <Row className="pr-check-open">{go}</Row>
      </Row>
      {opening.error ? <Notice tone="danger">{opening.error}</Notice> : null}
      {open ? <FailureBody id={detailId} load={load} /> : null}
    </Stack>
  );
}

/** The "›" at the end of a check that opens its run, as large as pr.css's 16px one. */
function Go() {
  if (WINGS_NATIVE) {
    return (
      <Svg viewBox="0 0 16 16" size={16} tone="muted" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M6.5 4.5 10 8l-3.5 3.5" />
      </Svg>
    );
  }
  return <Text aria-hidden="true" className="pr-check-go">›</Text>;
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
