// Small pieces the Issues pages share.

import { Badge, Button, Card, Dot, Row, Text, Time } from "wings-plugin-ui";
import { FLOW, lastSession, STATUS_LABEL, TYPE_LABEL, type Issue, type IssueType, type Status } from "./issues";

const TONE = { todo: "muted", in_progress: "info", in_review: "warn", done: undefined, canceled: "muted" } as const;

/** A dot for an issue's status. Decoration: the column or the text names it. */
export function StatusDot({ status }: { status: Status }) {
  return <Dot tone={TONE[status]} />;
}

const TYPE_PATH: Record<IssueType, React.ReactNode> = {
  task: <><rect x="2.5" y="2.5" width="11" height="11" rx="2.5" /><path d="m5.5 8.2 1.8 1.8 3.2-3.6" /></>,
  bug: <><ellipse cx="8" cy="9" rx="3" ry="4" /><path d="M5 9H2.5M11 9h2.5M5.5 5.5 4 4M10.5 5.5 12 4M5.5 12.5 4 14M10.5 12.5 12 14M6 5a2 2 0 0 1 4 0" /></>,
  feature: <path d="M8 2.2 9.6 6.4 13.8 8 9.6 9.6 8 13.8 6.4 9.6 2.2 8 6.4 6.4Z" />,
  epic: <path d="M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z" />,
};

/** A small icon for an issue's type, coloured by it. Decoration unless `label`: the card or page names the type in text too. */
export function TypeIcon({ type, label }: { type: IssueType; label?: boolean }) {
  return (
    <svg className="tk-type" data-type={type} viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" {...(label ? { role: "img", "aria-label": TYPE_LABEL[type] } : { "aria-hidden": true })}>
      <title>{TYPE_LABEL[type]}</title>
      {TYPE_PATH[type]}
    </svg>
  );
}

/** Click props that open something here, or in a new tab on a middle-click or Cmd/Ctrl+click. */
export const openProps = (open: (newTab: boolean) => void) => ({
  onClick: (e: React.MouseEvent) => open(e.metaKey || e.ctrlKey),
  onAuxClick: (e: React.MouseEvent) => {
    if (e.button !== 1) return;
    e.preventDefault();
    open(true);
  },
});

/** Marks an issue an agent has worked on, with the name of the session that touched it last. Nothing for one only you have touched. */
export function SessionMarker({ issue }: { issue: Issue }) {
  const last = lastSession(issue);
  if (!last) return null;
  return <Badge variant="outline" title={`Claude session ${last.name ?? last.id}`}>{last.name ? (last.name.length > 14 ? `${last.name.slice(0, 13)}…` : last.name) : "Claude"}</Badge>;
}

/** How many of an epic's or feature's children are done, as a short bar and "3/7". */
export function ChildProgress({ done, total }: { done: number; total: number }) {
  return (
    <span className="tk-child-progress" title={`${done} of ${total} child issues done`}>
      <span className="tk-progress" aria-hidden>
        <span style={{ width: `${(done / total) * 100}%` }} />
      </span>
      <span aria-hidden>{done}/{total}</span>
      <span className="w-visually-hidden">{done} of {total} child issues done</span>
    </span>
  );
}

/**
 * An issue as a card: key and the session marker, the title (a button that opens the issue), when it last changed and
 * buttons to move it a column. Alt+Left and Alt+Right on the title move it too. `progress` is an epic's or feature's
 * children done; `hideParent` leaves out the parent's key in a row that already names it.
 */
export function IssueCard({ issue, id, compact, progress, hideParent, onOpen, onMove }: { issue: Issue; id: string; compact?: boolean; progress?: { done: number; total: number }; hideParent?: boolean; onOpen: (newTab: boolean) => void; onMove: (by: 1 | -1) => void }) {
  const index = FLOW.indexOf(issue.status);
  const target = (by: 1 | -1) => STATUS_LABEL[FLOW[index + by] ?? issue.status];
  const movable = index >= 0;
  return (
    <Card compact={compact} className="tk-card" data-status={issue.status}>
      <Row gap="s" justify="between" className="tk-card-top">
        <Row gap="xs" className="tk-card-key">
          <TypeIcon type={issue.type} />
          <Text as="span" mono tone="muted" size="tiny">{issue.key}</Text>
          {issue.parent && !hideParent ? <Text as="span" mono tone="muted" size="tiny" className="tk-card-parent" title={`In ${issue.parent}`}>· {issue.parent}</Text> : null}
        </Row>
        <SessionMarker issue={issue} />
      </Row>
      <Button
        id={id}
        variant="ghost"
        className="tk-card-title"
        {...openProps(onOpen)}
        onKeyDown={(e) => {
          if (!movable || !e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
          e.preventDefault();
          onMove(e.key === "ArrowRight" ? 1 : -1);
        }}
      >
        <Text as="span" clamp>{issue.title}</Text>
      </Button>
      {progress?.total ? <ChildProgress {...progress} /> : null}
      <div className="tk-card-foot">
        <Time iso={new Date(issue.updatedAt).toISOString()} prefix={issue.status === "done" ? "Done " : "Updated "} />
        {movable ? (
          <span className="tk-moves">
            <Button size="xs" variant="ghost" disabled={index === 0} aria-label={`Move ${issue.key} to ${target(-1)}`} onClick={() => onMove(-1)}>←</Button>
            <Button size="xs" variant="ghost" disabled={index === FLOW.length - 1} aria-label={`Move ${issue.key} to ${target(1)}`} onClick={() => onMove(1)}>→</Button>
          </span>
        ) : null}
      </div>
    </Card>
  );
}
