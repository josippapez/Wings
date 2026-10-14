// The Issues tab without its data: the board for a list of issues. The tab page feeds it from storage, a fixture from made-up data.

import { Board, BoardColumn, Chip, Loading, Page, Summary, SummaryChip, Text, Toolbar } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { boardColumns, FLOW, STATUS_LABEL, type Issue, type Op } from "./issues";
import { IssueCard, StatusDot } from "./parts";

export interface BoardViewProps {
  project: WingsProject;
  issues: Issue[] | null;
  error: string | null;
  showCanceled: boolean;
  onShowCanceled: (show: boolean) => void;
  apply: (op: Op) => Promise<void>;
  send: (op: Op) => Promise<void>;
  onOpen: (issue: Issue) => void;
}

const EMPTY: Record<string, string> = { todo: "Nothing waiting", in_progress: "Nothing in progress", in_review: "Nothing in review", done: "Nothing finished yet", canceled: "Nothing canceled" };

export function BoardView({ project, issues, error, showCanceled, onShowCanceled, apply, send, onOpen }: BoardViewProps) {
  const count = (status: Issue["status"]) => issues?.filter((i) => i.status === status).length ?? 0;

  const move = (issue: Issue, by: 1 | -1) => {
    const next = FLOW[FLOW.indexOf(issue.status) + by];
    if (!next) return;
    void send({ kind: "update", key: issue.key, status: next }).then(() => document.getElementById(`issue-${issue.key}`)?.focus());
  };

  return (
    <Page
      flush={!!issues}
      toolbar={
        <Toolbar title={`Issues, ${project.name}`} actions={<AddIssue apply={apply} />}>
          {issues ? (
            <>
              <Summary aria-label="Issue counts">
                <SummaryChip>{count("todo")} to do</SummaryChip>
                <SummaryChip tone="info">{count("in_progress")} in progress</SummaryChip>
                <SummaryChip tone="warn">{count("in_review")} in review</SummaryChip>
                <SummaryChip tone="success">{count("done")} done</SummaryChip>
              </Summary>
              <Chip pressed={showCanceled} onClick={() => onShowCanceled(!showCanceled)}>Show canceled ({count("canceled")})</Chip>
            </>
          ) : null}
        </Toolbar>
      }
    >
      {error ? <Text tone="danger" role="alert" className="tk-error">{error}</Text> : null}
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : (
        <Board className="tk-board" label={`Issues for ${project.name}`}>
          {boardColumns(issues, showCanceled).map(({ status, issues: column }) => (
            <BoardColumn key={status} title={<span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>} count={column.length}>
              {column.length === 0 ? <p className="tk-column-empty">{EMPTY[status]}</p> : null}
              {column.map((i) => <IssueCard key={i.key} issue={i} id={`issue-${i.key}`} onOpen={() => onOpen(i)} onMove={(by) => move(i, by)} />)}
            </BoardColumn>
          ))}
        </Board>
      )}
    </Page>
  );
}
