// The Issues tab without its data: the board for a list of issues. The tab page feeds it from storage, a fixture from made-up data.

import { useState } from "react";
import { Board, BoardCell, BoardColumn, BoardLane, BoardLanes, Button, Chip, Chips, Disclosure, Icon, List, ListButton, ListItem, Loading, Page, SearchField, Text, Time, Toolbar } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { canParent, FLOW, STATUS_LABEL, STATUSES, type Issue, type Op } from "../lib/domain";
import { iso } from "../lib/format";
import { boardColumns, boardLanes, issueTree, type Lane } from "../lib/orderings";
import { childrenOf, progress } from "../lib/relations";
import { matchesQuery } from "../lib/search";
import { ChildProgress, DeleteConfirm, ErrorText, IssueCard, isDeleteKey, openProps, PriorityIcon, SessionMarker, StatusDot, StatusSummary, TypeIcon, type Send } from "./parts";

export interface BoardViewProps {
  project: WingsProject;
  issues: Issue[] | null;
  error: string | null;
  showCanceled: boolean;
  onShowCanceled: (show: boolean) => void;
  /** Columns of cards, or rows grouped by status like Linear's list. */
  layout: Layout;
  onLayout: (layout: Layout) => void;
  /** Rows by parent epic or feature, like Linear's sub-grouping. */
  byParent: boolean;
  onByParent: (on: boolean) => void;
  /** The rows folded shut: their parent's key, "" for the issues with no parent, or `status:<status>` for a list group. */
  folded: string[];
  onFold: (lane: string, open: boolean) => void;
  /** Words an issue's key, title, description or log must all contain to be shown. */
  query: string;
  onQuery: (query: string) => void;
  apply: (op: Op) => Promise<void>;
  send: Send;
  onOpen: (issue: Issue, newTab: boolean) => void;
  /** The key of an issue to start out asking to delete, for a fixture. */
  deleting?: string;
}

export type Layout = "board" | "list";
const LAYOUTS = [{ value: "board" as const, label: "Board" }, { value: "list" as const, label: "List" }];
/** The list's groups, in Linear's order: what's closest to done first. */
const LIST_ORDER: Issue["status"][] = ["in_review", "in_progress", "todo", "done", "canceled"];

const EMPTY: Record<string, string> = { todo: "Nothing waiting", in_progress: "Nothing in progress", in_review: "Nothing in review", done: "Nothing finished yet", canceled: "Nothing canceled" };

export function BoardView({ project, issues: all, error, showCanceled, onShowCanceled, layout, onLayout, byParent, onByParent, folded, onFold, query, onQuery, apply, send, onOpen, deleting: asking }: BoardViewProps) {
  // Summary counts and the Group by parent chip stay on every issue; what's below the toolbar is what matches. Each
  // issue's text is lowercased once (see `matchesQuery`), so a keystroke is a substring test per issue.
  const searching = !!query.trim();
  const issues = all && searching ? all.filter((i) => matchesQuery(i, query)) : all;
  const count = (status: Issue["status"]) => all?.filter((i) => i.status === status).length ?? 0;
  const shownCount = (status: Issue["status"]) => issues?.filter((i) => i.status === status).length ?? 0;
  const hasParents = !!all?.some((i) => i.parent);
  const lanes = issues && hasParents && byParent && layout === "board" ? boardLanes(issues, showCanceled) : null;
  const [deleting, setDeleting] = useState<string | null>(asking ?? null);
  const doomed = deleting ? issues?.find((i) => i.key === deleting) : undefined;

  const move = (issue: Issue, by: 1 | -1) => {
    const next = FLOW[FLOW.indexOf(issue.status) + by];
    if (!next) return;
    void send({ kind: "update", key: issue.key, status: next }).then(() => document.getElementById(`issue-${issue.key}`)?.focus());
  };
  const card = (i: Issue, inLane: boolean) => (
    <IssueCard
      key={i.key}
      issue={i}
      id={`issue-${i.key}`}
      progress={issues && canParent(i.type) ? progress(childrenOf(issues, i.key)) : undefined}
      hideParent={inLane && !!i.parent}
      onOpen={(newTab) => onOpen(i, newTab)}
      onMove={(by) => move(i, by)}
      onDelete={() => setDeleting(i.key)}
    />
  );

  return (
    <Page
      flush={!!issues && layout === "board"}
      toolbar={
        <Toolbar className="tk-board-toolbar" title={`Issues, ${project.name}`} actions={<AddIssue apply={apply} />}>
          {all ? (
            <>
              <StatusSummary issues={all} />
              <SearchField className="tk-search" label="Search issues" placeholder="Search issues" value={query} onChange={(e) => onQuery(e.target.value)} />
              <Chips label="View" options={LAYOUTS} value={layout} onChange={onLayout} />
              {hasParents ? <Chip pressed={byParent} onClick={() => onByParent(!byParent)}>Group by parent</Chip> : null}
              <Chip pressed={showCanceled} onClick={() => onShowCanceled(!showCanceled)}>Show canceled ({count("canceled")})</Chip>
            </>
          ) : null}
        </Toolbar>
      }
    >
      {error ? <ErrorText className="tk-error">{error}</ErrorText> : null}
      {doomed && issues ? (
        <div className="tk-confirm-wrap" data-layout={layout}>
          <DeleteConfirm issue={doomed} issues={issues} onCancel={() => setDeleting(null)} onDelete={(children) => apply({ kind: "remove", key: doomed.key, children }).then(() => setDeleting(null))} />
        </div>
      ) : null}
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : searching && issues.length === 0 ? (
        <div className="tk-confirm-wrap" data-layout={layout}><Text tone="muted" role="status">No issues match "{query.trim()}".</Text></div>
      ) : layout === "list" ? (
        <IssueList project={project} issues={issues} byParent={hasParents && byParent} showCanceled={showCanceled} folded={folded} onFold={onFold} onOpen={onOpen} onDelete={(i) => setDeleting(i.key)} />
      ) : lanes ? (
        <BoardLanes
          label={`Issues for ${project.name}, by parent`}
          columns={(showCanceled ? STATUSES : FLOW).map((status) => ({ id: status, title: <span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>, count: shownCount(status) }))}
        >
          {lanes.map((lane) => (
            <ParentLane key={lane.parent?.key ?? ""} lane={lane} issues={issues} open={!folded.includes(lane.parent?.key ?? "")} onOpenChange={(open) => onFold(lane.parent?.key ?? "", open)} onOpen={onOpen} card={(i) => card(i, true)} />
          ))}
        </BoardLanes>
      ) : (
        <Board className="tk-board" label={`Issues for ${project.name}`}>
          {boardColumns(issues, showCanceled).map(({ status, issues: column }) => (
            <BoardColumn key={status} title={<span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>} count={column.length}>
              {column.length === 0 ? <p className="tk-column-empty">{EMPTY[status]}</p> : null}
              {column.map((i) => card(i, false))}
            </BoardColumn>
          ))}
        </Board>
      )}
    </Page>
  );
}

/** One row: the parent's status, type, key and title (which opens it) and how many of its children are done, over its cards. */
function ParentLane({ lane, issues, open, onOpenChange, onOpen, card }: { lane: Lane; issues: Issue[]; open: boolean; onOpenChange: (open: boolean) => void; onOpen: (issue: Issue, newTab: boolean) => void; card: (issue: Issue) => React.ReactNode }) {
  const { parent } = lane;
  const done = parent ? progress(childrenOf(issues, parent.key)) : null;
  return (
    <BoardLane
      label={parent ? `${parent.key} ${parent.title}` : "No parent"}
      title={
        parent ? (
          <span className="tk-lane-title">
            <StatusDot status={parent.status} />
            <TypeIcon type={parent.type} label />
            <Text as="span" mono tone="muted" size="tiny">{parent.key}</Text>
            <Button variant="ghost" size="xs" className="tk-lane-open" {...openProps((newTab) => onOpen(parent, newTab))}>
              <span className="tk-link-title">{parent.title}</span>
            </Button>
          </span>
        ) : (
          <span className="tk-lane-none">No parent</span>
        )
      }
      count={lane.count}
      actions={
        parent ? (
          <>
            <Text tone="muted" size="small">{STATUS_LABEL[parent.status]}</Text>
            {done?.total ? <ChildProgress {...done} /> : null}
          </>
        ) : null
      }
      open={open}
      onOpenChange={onOpenChange}
    >
      {lane.columns.map(({ status, issues: cell }) => (
        <BoardCell key={status} label={STATUS_LABEL[status]}>
          {cell.map(card)}
        </BoardCell>
      ))}
    </BoardLane>
  );
}

/** Issues as rows like Linear's list: by parent, each epic over its children indented under it, or by status. Groups fold. */
function IssueList({ project, issues, byParent, showCanceled, folded, onFold, onOpen, onDelete }: { project: WingsProject; issues: Issue[]; byParent: boolean; showCanceled: boolean; folded: string[]; onFold: (group: string, open: boolean) => void; onOpen: (issue: Issue, newTab: boolean) => void; onDelete: (issue: Issue) => void }) {
  const byKey = new Map(issues.map((i) => [i.key, i]));
  const row = (i: Issue, depth?: number) => <IssueRow key={i.key} issue={i} issues={issues} parent={depth === undefined && i.parent ? byKey.get(i.parent) : undefined} depth={depth} onOpen={onOpen} onDelete={onDelete} />;
  const isOpen = (group: string) => !folded.includes(group);
  if (byParent) {
    const { groups, loose } = issueTree(issues, showCanceled);
    if (groups.length + loose.length === 0) return <Text tone="muted">No issues for {project.name} yet.</Text>;
    return (
      <div className="tk-list">
        {groups.map(({ root, rows }) => (
          <section key={root.key} className="tk-tree-group" aria-label={`${root.key} ${root.title}`}>
            <div className="tk-tree-head">
              <Button variant="ghost" size="icon-xs" className="tk-fold" aria-label={root.title} aria-expanded={isOpen(root.key)} onClick={() => onFold(root.key, !isOpen(root.key))}>
                <Icon name="chevron-down" />
              </Button>
              <ListButton className="tk-row tk-tree-root" data-status={root.status} {...openProps((newTab) => onOpen(root, newTab))} onKeyDown={(e) => isDeleteKey(e) && (e.preventDefault(), onDelete(root))} aria-label={`Open ${root.key}: ${root.title}`}>
                <PriorityIcon priority={root.priority} label />
                <TypeIcon type={root.type} label />
                <span className="tk-key">{root.key}</span>
                <Text as="span" grow truncate className="tk-row-title">{root.title}</Text>
                <ChildProgress {...progress(childrenOf(issues, root.key))} />
                <span className="tk-row-status"><StatusDot status={root.status} />{STATUS_LABEL[root.status]}</span>
                <span className="tk-row-time" />
              </ListButton>
              <DeleteButton issue={root} onDelete={onDelete} />
            </div>
            {isOpen(root.key) ? <List compact aria-label={`Under ${root.key}`}>{rows.map(({ issue, depth }) => row(issue, depth))}</List> : null}
          </section>
        ))}
        {loose.length ? (
          <Disclosure className="tk-list-group" open={isOpen("")} onOpenChange={(open) => onFold("", open)} summary={<span className="tk-col-title">No parent<span className="tk-count">{loose.length}</span></span>}>
            <List compact aria-label="No parent">{loose.map((i) => row(i, 0))}</List>
          </Disclosure>
        ) : null}
      </div>
    );
  }
  const groups = boardColumns(issues, showCanceled)
    .filter((g) => g.issues.length > 0)
    .sort((a, b) => LIST_ORDER.indexOf(a.status) - LIST_ORDER.indexOf(b.status));
  if (groups.length === 0) return <Text tone="muted">No issues for {project.name} yet.</Text>;
  return (
    <div className="tk-list">
      {groups.map(({ status, issues: rows }) => (
        <Disclosure
          key={status}
          className="tk-list-group"
          open={isOpen(`status:${status}`)}
          onOpenChange={(open) => onFold(`status:${status}`, open)}
          summary={<span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}<span className="tk-count">{rows.length}</span></span>}
        >
          <List compact aria-label={STATUS_LABEL[status]}>{rows.map((i) => row(i))}</List>
        </Disclosure>
      ))}
    </div>
  );
}

/**
 * One issue as a row: key, type and title, then its parent (in the status list), its children's progress, the Claude
 * session marker and when it changed. With a `depth` it sits in the tree by parent: indented, with its status shown.
 * Delete on the row, or its trash button, asks to delete it.
 */
function IssueRow({ issue: i, issues, parent, depth, onOpen, onDelete }: { issue: Issue; issues: Issue[]; parent?: Issue; depth?: number; onOpen: (issue: Issue, newTab: boolean) => void; onDelete: (issue: Issue) => void }) {
  const done = canParent(i.type) ? progress(childrenOf(issues, i.key)) : null;
  return (
    <ListItem className="tk-row-item">
      <ListButton className="tk-row" data-status={i.status} style={depth ? { paddingLeft: `${36 + (depth - 1) * 24}px` } : undefined} {...openProps((newTab) => onOpen(i, newTab))} onKeyDown={(e) => isDeleteKey(e) && (e.preventDefault(), onDelete(i))} aria-label={`Open ${i.key}: ${i.title}`}>
        {depth && depth > 1 ? <span className="tk-tree-elbow" aria-hidden>└</span> : null}
        <PriorityIcon priority={i.priority} label />
        <span className="tk-key">{i.key}</span>
        <TypeIcon type={i.type} />
        <Text as="span" grow truncate className="tk-row-title">{i.title}</Text>
        {parent ? <span className="tk-row-parent" title={`In ${parent.key} ${parent.title}`}><TypeIcon type={parent.type} /><span className="tk-link-title">{parent.title}</span></span> : null}
        {done?.total ? <ChildProgress {...done} /> : null}
        <SessionMarker issue={i} />
        {depth !== undefined ? <span className="tk-row-status"><StatusDot status={i.status} />{STATUS_LABEL[i.status]}</span> : null}
        <Time className="tk-row-time" iso={iso(i.updatedAt)} />
      </ListButton>
      <DeleteButton issue={i} onDelete={onDelete} />
    </ListItem>
  );
}

/** A row's trash button, over its right end where the time is, shown while you're on the row. */
function DeleteButton({ issue, onDelete }: { issue: Issue; onDelete: (issue: Issue) => void }) {
  return <Button size="icon-xs" variant="ghost" className="tk-row-delete" aria-label={`Delete ${issue.key}`} title="Delete (Del)" onClick={() => onDelete(issue)}><Icon name="trash" /></Button>;
}
