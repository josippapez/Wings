// Small pieces the three Tasks pages share.

import type { ReactNode } from "react";
import { Button, Card, Dot, Text, Time } from "wings-plugin-ui";
import { STATUSES, STATUS_LABEL, type Status, type Task } from "./tasks";

const TONE = { todo: "muted", doing: "info", done: undefined } as const;

/** A dot for a task's status: grey to do, blue doing, green done. Decoration, the column or the text names it. */
export function StatusDot({ status }: { status: Status }) {
  return <Dot tone={TONE[status]} />;
}

/**
 * A task as a card: status dot and title (a button that opens it), branch, the start of its notes, when it last
 * changed, and buttons to move it a column. Alt+Left and Alt+Right on the title move it too. `children` go under the head.
 */
export function TaskCard({ task, id, compact, expanded, onOpen, onMove, children }: { task: Task; id: string; compact?: boolean; expanded?: boolean; onOpen: () => void; onMove: (by: 1 | -1) => void; children?: ReactNode }) {
  const index = STATUSES.indexOf(task.status);
  const move = (by: 1 | -1) => `Move "${task.title}" to ${STATUS_LABEL[STATUSES[index + by] ?? task.status]}`;
  return (
    <Card compact={compact} className="tk-card" data-status={task.status}>
      <div className="tk-card-head">
        <StatusDot status={task.status} />
        <Button
          id={id}
          variant="ghost"
          className="tk-card-title"
          aria-expanded={expanded}
          onClick={onOpen}
          onKeyDown={(e) => {
            if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
            e.preventDefault();
            onMove(e.key === "ArrowRight" ? 1 : -1);
          }}
        >
          <Text as="span" clamp>{task.title}</Text>
        </Button>
      </div>
      {task.links?.branch ? <Text tone="muted" size="tiny" mono truncate title={task.links.branch}>{task.links.branch}</Text> : null}
      {!expanded && task.notes ? <p className="tk-notes">{task.notes}</p> : null}
      {children}
      <div className="tk-card-foot">
        <Time iso={new Date(task.updatedAt).toISOString()} prefix={task.status === "done" ? "Done " : "Updated "} />
        <span className="tk-moves">
          <Button size="xs" variant="ghost" disabled={index === 0} aria-label={move(-1)} onClick={() => onMove(-1)}>←</Button>
          <Button size="xs" variant="ghost" disabled={index === STATUSES.length - 1} aria-label={move(1)} onClick={() => onMove(1)}>→</Button>
        </span>
      </div>
    </Card>
  );
}
