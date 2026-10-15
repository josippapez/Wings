// The Checks tab: every check or policy of the pull request, grouped by how it stands. It draws what it's given; a
// failed check's log is asked for (`onLoadFailure`) only when the row is opened, and shown as plain text.

import { useMemo, useRef, useState } from "react";
import { ago, Button, Text } from "wings-plugin-ui";
import { groupChecks, durationText, hasLog, logTail, withUnresolved, type Group, type GroupId } from "../lib/checklist";
import type { CheckItem, CheckState, FailureDetail, Failing, Status } from "../lib/types";

const STATE_WORD: Record<CheckState, string> = { failed: "Failed", running: "Running", queued: "Queued", passed: "Passed", skipped: "Not run" };
const STATE_MARK: Record<CheckState, string> = { failed: "✕", running: "●", queued: "○", passed: "✓", skipped: "–" };

export interface ChecksViewProps {
  status: Status;
  /** Open comment threads, once they have loaded; a comment requirement says how many. */
  unresolved: number | null;
  onLoadFailure?: (f: Failing) => Promise<FailureDetail[]>;
}

type Load = { state: "loading" } | { state: "error"; message: string } | { state: "done"; details: FailureDetail[] };

export function ChecksView({ status, unresolved, onLoadFailure }: ChecksViewProps) {
  const items = useMemo(() => withUnresolved(status.items ?? [], unresolved), [status.items, unresolved]);
  const groups = useMemo(() => groupChecks(items), [items]);
  const [open, setOpen] = useState<Partial<Record<GroupId, boolean>>>({});
  const [loads, setLoads] = useState<Record<string, Load>>({});
  const asked = useRef(new Set<string>());

  if (!items.length) {
    const { passed, failed, running } = status.checks;
    return <Text tone="muted">{passed + failed + running ? "This provider didn't list the checks by name." : "No checks reported."}</Text>;
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
    <div className="pr-checks" data-testid="pr-checks-view">
      {groups.map((g) => (
        <GroupSection key={g.id} group={g} open={open[g.id] ?? (g.id === "failed" || g.id === "active")} onToggle={() => setOpen((o) => ({ ...o, [g.id]: !(o[g.id] ?? (g.id === "failed" || g.id === "active")) }))} loadKey={loadKey} loads={loads} onAsk={ask} canLoad={Boolean(onLoadFailure)} />
      ))}
    </div>
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
}

function GroupSection({ group, open, onToggle, loadKey, loads, onAsk, canLoad }: SectionProps) {
  const listId = `pr-checks-${group.id}`;
  return (
    <section className="pr-check-group" data-group={group.id} aria-label={group.label}>
      <button type="button" className="pr-check-group-head" aria-expanded={open} aria-controls={open ? listId : undefined} onClick={onToggle} data-testid={`pr-checks-${group.id}-toggle`}>
        <span aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</span>
        <strong>{group.label}</strong>
        <span className="pr-check-count">{group.items.length}</span>
      </button>
      {open ? (
        <ul id={listId} className="pr-check-list" aria-label={`${group.label} checks`}>
          {group.items.map((c) => (
            <CheckRow key={c.id} item={c} load={loads[loadKey(c)]} onAsk={onAsk} canLoad={canLoad} />
          ))}
        </ul>
      ) : null}
    </section>
  );
}

function CheckRow({ item: c, load, onAsk, canLoad }: { item: CheckItem; load: Load | undefined; onAsk: (c: CheckItem) => void; canLoad: boolean }) {
  const [open, setOpen] = useState(false);
  const expandable = canLoad && c.state === "failed" && hasLog(c.failing);
  const url = c.url?.startsWith("https://") ? c.url : null;
  const time = c.at ? ago(c.at) : "";
  const took = durationText(c.seconds);
  const detailId = `pr-check-detail-${c.id.replace(/[^\w-]/g, "_")}`;
  const main = (
    <>
      <span className="pr-check-mark" data-state={c.state} role="img" aria-label={STATE_WORD[c.state]}>{STATE_MARK[c.state]}</span>
      <span className="pr-check-text">
        <span className="pr-check-line">
          <span className="pr-check-name">{c.name}</span>
          {c.required === true ? <span className="pr-tag" data-tone="required">Required</span> : c.required === false ? <span className="pr-tag">Optional</span> : null}
          {c.kind && c.kind !== c.name ? <Text tone="muted" size="tiny" as="span">{c.kind}</Text> : null}
        </span>
        {c.note ? <Text tone={c.state === "failed" ? "danger" : "muted"} size="small" as="span" className="pr-check-note">{c.note}</Text> : null}
      </span>
      {expandable ? <span aria-hidden="true" className="pr-chevron" data-open={open ? "" : undefined}>▾</span> : null}
    </>
  );
  return (
    <li className="pr-check" data-state={c.state} data-testid="pr-check">
      <div className="pr-check-row">
        {expandable ? (
          <button
            type="button"
            className="pr-check-main"
            aria-expanded={open}
            aria-controls={open ? detailId : undefined}
            onClick={() => {
              setOpen((o) => !o);
              if (!open) onAsk(c);
            }}
          >
            {main}
          </button>
        ) : (
          <div className="pr-check-main">{main}</div>
        )}
        {time || took ? (
          <Text tone="muted" size="small" as="span" className="pr-check-time" title={c.at ? new Date(c.at).toLocaleString() : undefined}>
            {[time, took].filter(Boolean).join(" · ")}
          </Text>
        ) : null}
        <span className="pr-check-open">{url ? <Button size="sm" variant="ghost" aria-label={`Open ${c.name}`} onClick={() => void wings.openUrl(url)}>Open</Button> : null}</span>
      </div>
      {open ? <FailureBody id={detailId} load={load} /> : null}
    </li>
  );
}

function FailureBody({ id, load }: { id: string; load: Load | undefined }) {
  return (
    <div id={id} className="pr-check-detail" aria-live="polite">
      {!load || load.state === "loading" ? <Text tone="muted" size="small">Loading the log…</Text> : null}
      {load?.state === "error" ? <Text tone="danger" size="small" role="alert">{load.message}</Text> : null}
      {load?.state === "done" ? load.details.map((d, i) => <Detail key={i} d={d} />) : null}
    </div>
  );
}

/** The error is at the end of a log, so it opens scrolled there. */
const endOf = (el: HTMLElement | null) => {
  if (el) el.scrollTop = el.scrollHeight;
};

function Detail({ d }: { d: FailureDetail }) {
  const tail = d.log ? logTail(d.log) : null;
  return (
    <div className="pr-fail">
      <Text size="small" as="span"><strong>{d.step ?? d.check}</strong>{d.summary ? `: ${d.summary}` : ""}</Text>
      {tail ? (
        <>
          {tail.hidden ? <Text tone="muted" size="tiny">Showing the last {tail.text.split("\n").length} of {tail.text.split("\n").length + tail.hidden} lines.</Text> : null}
          <pre className="pr-log" ref={endOf} tabIndex={0} aria-label={`Log of ${d.step ?? d.check ?? "the check"}`}><code>{tail.text}</code></pre>
        </>
      ) : (
        <Text tone="muted" size="tiny">No log for this step.</Text>
      )}
    </div>
  );
}
