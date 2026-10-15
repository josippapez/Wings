// The Issues tab without its data: the board for a list of issues. The tab page feeds it from storage, a fixture from made-up data.

import { Board, BoardCell, BoardColumn, BoardLane, BoardLanes, Button, Chip, Loading, Page, Summary, SummaryChip, Text, Toolbar } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { boardColumns, boardLanes, canParent, childrenOf, FLOW, progress, STATUS_LABEL, STATUSES, type Issue, type Lane, type Op } from "./issues";
import { ChildProgress, IssueCard, openProps, StatusDot, TypeIcon } from "./parts";

export interface BoardViewProps {
  project: WingsProject;
  issues: Issue[] | null;
  error: string | null;
  showCanceled: boolean;
  onShowCanceled: (show: boolean) => void;
  /** Rows by parent epic or feature, like Linear's sub-grouping. */
  byParent: boolean;
  onByParent: (on: boolean) => void;
  /** The rows folded shut: their parent's key, or "" for the issues with no parent. */
  folded: string[];
  onFold: (lane: string, open: boolean) => void;
  apply: (op: Op) => Promise<void>;
  send: (op: Op) => Promise<void>;
  onOpen: (issue: Issue, newTab: boolean) => void;
}

const EMPTY: Record<string, string> = { todo: "Nothing waiting", in_progress: "Nothing in progress", in_review: "Nothing in review", done: "Nothing finished yet", canceled: "Nothing canceled" };

export function BoardView({ project, issues, error, showCanceled, onShowCanceled, byParent, onByParent, folded, onFold, apply, send, onOpen }: BoardViewProps) {
  const count = (status: Issue["status"]) => issues?.filter((i) => i.status === status).length ?? 0;
  const hasParents = !!issues?.some((i) => i.parent);
  const lanes = issues && hasParents && byParent ? boardLanes(issues, showCanceled) : null;

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
    />
  );

  return (
    <Page
      flush={!!issues}
      toolbar={
        <Toolbar className="tk-board-toolbar" title={`Issues, ${project.name}`} actions={<AddIssue apply={apply} />}>
          {issues ? (
            <>
              <Summary aria-label="Issue counts">
                <SummaryChip>{count("todo")} to do</SummaryChip>
                <SummaryChip tone="info">{count("in_progress")} in progress</SummaryChip>
                <SummaryChip tone="warn">{count("in_review")} in review</SummaryChip>
                <SummaryChip tone="success">{count("done")} done</SummaryChip>
              </Summary>
              {hasParents ? <Chip pressed={byParent} onClick={() => onByParent(!byParent)}>Group by parent</Chip> : null}
              <Chip pressed={showCanceled} onClick={() => onShowCanceled(!showCanceled)}>Show canceled ({count("canceled")})</Chip>
            </>
          ) : null}
        </Toolbar>
      }
    >
      {error ? <Text tone="danger" role="alert" className="tk-error">{error}</Text> : null}
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : lanes ? (
        <BoardLanes
          className="tk-lanes"
          label={`Issues for ${project.name}, by parent`}
          columns={(showCanceled ? STATUSES : FLOW).map((status) => ({ id: status, title: <span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>, count: count(status) }))}
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
