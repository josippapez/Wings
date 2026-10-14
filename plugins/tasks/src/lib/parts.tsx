// Small pieces the Issues pages share.

import { Badge, Button, Card, Dot, Row, Text, Time } from "wings-plugin-ui";
import { FLOW, lastSession, STATUS_LABEL, type Issue, type Status } from "./issues";

const TONE = { todo: "muted", in_progress: "info", in_review: "warn", done: undefined, canceled: "muted" } as const;

/** A dot for an issue's status. Decoration: the column or the text names it. */
export function StatusDot({ status }: { status: Status }) {
  return <Dot tone={TONE[status]} />;
}

/** Marks an issue an agent has worked on, with the name of the session that touched it last. Nothing for one only you have touched. */
export function SessionMarker({ issue }: { issue: Issue }) {
  const last = lastSession(issue);
  if (!last) return null;
  return <Badge variant="outline" title={`Claude session ${last.name ?? last.id}`}>{last.name ? (last.name.length > 14 ? `${last.name.slice(0, 13)}…` : last.name) : "Claude"}</Badge>;
}

/**
 * An issue as a card: key and the session marker, the title (a button that opens the issue), when it last changed and
 * buttons to move it a column. Alt+Left and Alt+Right on the title move it too.
 */
export function IssueCard({ issue, id, compact, onOpen, onMove }: { issue: Issue; id: string; compact?: boolean; onOpen: () => void; onMove: (by: 1 | -1) => void }) {
  const index = FLOW.indexOf(issue.status);
  const target = (by: 1 | -1) => STATUS_LABEL[FLOW[index + by] ?? issue.status];
  const movable = index >= 0;
  return (
    <Card compact={compact} className="tk-card" data-status={issue.status}>
      <Row gap="s" justify="between" className="tk-card-top">
        <Text as="span" mono tone="muted" size="tiny">{issue.key}</Text>
        <SessionMarker issue={issue} />
      </Row>
      <Button
        id={id}
        variant="ghost"
        className="tk-card-title"
        onClick={onOpen}
        onKeyDown={(e) => {
          if (!movable || !e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
          e.preventDefault();
          onMove(e.key === "ArrowRight" ? 1 : -1);
        }}
      >
        <Text as="span" clamp>{issue.title}</Text>
      </Button>
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
