// Small pieces the Issues pages share.

import type { ComponentProps } from "react";
import { Badge, Button, Card, Dot, Row, Summary, SummaryChip, Text, Time } from "wings-plugin-ui";
import { lastSession } from "../lib/actor";
import { FLOW, PRIORITY_LABEL, STATUS_LABEL, TYPE_LABEL, type Issue, type IssueType, type Op, type Priority, type Status } from "../lib/domain";
import { iso } from "../lib/format";
import { descendantsOf } from "../lib/relations";

/** Sends an operation to main. */
export type Send = (op: Op) => Promise<void>;

/** An error from the last change: in the danger colour, and announced to a screen reader. */
export function ErrorText(props: Omit<ComponentProps<typeof Text>, "tone" | "role">) {
  return <Text tone="danger" role="alert" {...props} />;
}

/** How many issues are to do, in progress, in review and done. */
export function StatusSummary({ issues }: { issues: Issue[] }) {
  const count = (status: Status) => issues.filter((i) => i.status === status).length;
  return (
    <Summary aria-label="Issue counts">
      <SummaryChip>{count("todo")} to do</SummaryChip>
      <SummaryChip tone="info">{count("in_progress")} in progress</SummaryChip>
      <SummaryChip tone="warn">{count("in_review")} in review</SummaryChip>
      <SummaryChip tone="success">{count("done")} done</SummaryChip>
    </Summary>
  );
}

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

/**
 * Linear's priority icon: three bars filled by level, an exclamation mark in a square for urgent, a dash for none. Named
 * for a screen reader with `label`, otherwise decoration next to text that says it.
 */
export function PriorityIcon({ priority = "none", label }: { priority?: Priority; label?: boolean }) {
  const filled = { urgent: 3, high: 3, medium: 2, low: 1, none: 0 }[priority];
  return (
    <svg className="tk-priority" data-priority={priority} viewBox="0 0 16 16" width="14" height="14" {...(label ? { role: "img", "aria-label": PRIORITY_LABEL[priority] } : { "aria-hidden": true })}>
      <title>{PRIORITY_LABEL[priority]}</title>
      {priority === "urgent" ? (
        <>
          <rect x="1.5" y="1.5" width="13" height="13" rx="3" fill="currentColor" />
          <path d="M8 4.5v4.5M8 11.2v.3" stroke="var(--wings-primary-fg)" strokeWidth="1.8" strokeLinecap="round" />
        </>
      ) : priority === "none" ? (
        <path d="M3 8h2M7 8h2M11 8h2" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      ) : (
        [0, 1, 2].map((i) => <rect key={i} x={2 + i * 4.5} y={10 - i * 3.5} width="3" height={4 + i * 3.5} rx="1" fill="currentColor" opacity={i < filled ? 1 : 0.3} />)
      )}
    </svg>
  );
}

/** Lucide's trash-2, for the buttons that ask to delete an issue. */
export function TrashIcon() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 6h18M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2M10 11v6M14 11v6" />
    </svg>
  );
}

/** True for the keys that ask to delete the focused issue: Delete, or Backspace on a Mac keyboard that has no Delete. */
export const isDeleteKey = (e: React.KeyboardEvent) => (e.key === "Delete" || e.key === "Backspace") && !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey;

/**
 * Asks before deleting `issue`. With child issues (at any depth) it offers to delete only this one, which leaves them
 * without a parent, or all of them. Focus starts on the delete that keeps the most, so Enter never takes more than asked.
 * Escape keeps it.
 */
export function DeleteConfirm({ issue, issues, onCancel, onDelete }: { issue: Issue; issues: Issue[]; onCancel: () => void; onDelete: (children: boolean) => void }) {
  const below = descendantsOf(issues, issue.key).length;
  const children = below === 1 ? "1 child issue" : `${below} child issues`;
  return (
    <Card role="alertdialog" aria-label={`Delete ${issue.key}`} onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), onCancel())}>
      <Row gap="m" wrap justify="between">
        <Text size="small">
          Delete <strong>{issue.key}</strong> {issue.title}?{below ? ` It has ${children}.` : ""} This removes {below ? "what you delete" : "the issue"} and its log. Keys are never used again.
        </Text>
        <Row gap="s" wrap>
          <Button variant="ghost" size="sm" onClick={onCancel}>Keep it</Button>
          {below ? (
            <>
              <Button variant="outline" size="sm" autoFocus onClick={() => onDelete(false)}>Delete only {issue.key}</Button>
              <Button variant="destructive" size="sm" onClick={() => onDelete(true)}>Delete with {children}</Button>
            </>
          ) : (
            <Button variant="destructive" size="sm" autoFocus onClick={() => onDelete(false)}>Delete {issue.key}</Button>
          )}
        </Row>
      </Row>
    </Card>
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
 * buttons to move it a column or delete it. Alt+Left and Alt+Right on the title move it too, Delete asks to delete it. `progress` is an epic's or feature's
 * children done; `hideParent` leaves out the parent's key in a row that already names it.
 */
export function IssueCard({ issue, id, compact, progress, hideParent, onOpen, onMove, onDelete }: { issue: Issue; id: string; compact?: boolean; progress?: { done: number; total: number }; hideParent?: boolean; onOpen: (newTab: boolean) => void; onMove: (by: 1 | -1) => void; onDelete?: () => void }) {
  const index = FLOW.indexOf(issue.status);
  const target = (by: 1 | -1) => STATUS_LABEL[FLOW[index + by] ?? issue.status];
  const movable = index >= 0;
  return (
    <Card compact={compact} className="tk-card" data-status={issue.status}>
      <Row gap="s" justify="between" className="tk-card-top">
        <Row gap="xs" className="tk-card-key">
          {issue.priority ? <PriorityIcon priority={issue.priority} label /> : null}
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
          if (onDelete && isDeleteKey(e)) return (e.preventDefault(), onDelete());
          if (!movable || !e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
          e.preventDefault();
          onMove(e.key === "ArrowRight" ? 1 : -1);
        }}
      >
        <Text as="span" clamp>{issue.title}</Text>
      </Button>
      {progress?.total ? <ChildProgress {...progress} /> : null}
      <div className="tk-card-foot">
        <Time iso={iso(issue.updatedAt)} prefix={issue.status === "done" ? "Done " : "Updated "} />
        {movable || onDelete ? (
          <span className="tk-moves">
            {onDelete ? <Button size="icon-xs" variant="ghost" className="tk-delete" aria-label={`Delete ${issue.key}`} title="Delete (Del)" onClick={onDelete}><TrashIcon /></Button> : null}
            {movable ? (
              <>
                <Button size="xs" variant="ghost" disabled={index === 0} aria-label={`Move ${issue.key} to ${target(-1)}`} onClick={() => onMove(-1)}>←</Button>
                <Button size="xs" variant="ghost" disabled={index === FLOW.length - 1} aria-label={`Move ${issue.key} to ${target(1)}`} onClick={() => onMove(1)}>→</Button>
              </>
            ) : null}
          </span>
        ) : null}
      </div>
    </Card>
  );
}
