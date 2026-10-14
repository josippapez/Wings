// The Issues block of a Project page without its data: counts and a compact list of the open issues.

import { Button, Empty, List, ListButton, ListItem, Loading, Row, Stack, Summary, SummaryChip, Text } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { openIssues, STATUS_LABEL, type Issue, type Op } from "./issues";
import { SessionMarker, StatusDot, TypeIcon } from "./parts";

/** The block lists this many open issues; the board has the rest. */
export const BLOCK_ROWS = 8;

export interface BlockViewProps {
  project: WingsProject;
  issues: Issue[] | null;
  error: string | null;
  apply: (op: Op) => Promise<void>;
  openBoard: () => void;
  openIssue: (issue: Issue) => void;
}

export function BlockView({ project, issues, error, apply, openBoard, openIssue }: BlockViewProps) {
  const count = (status: Issue["status"]) => issues?.filter((i) => i.status === status).length ?? 0;
  const open = issues ? openIssues(issues) : [];
  return (
    <Stack gap="s">
      <Row gap="m" justify="between" wrap>
        <AddIssue apply={apply} />
        <Button variant="outline" size="sm" onClick={openBoard}>Open board</Button>
      </Row>
      {error ? <Text tone="danger" role="alert" size="small">{error}</Text> : null}
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : issues.length === 0 ? (
        <Empty title="No issues yet" description="Add one above, or ask Claude to keep track of its work here." />
      ) : (
        <>
          <Summary aria-label="Issue counts">
            <SummaryChip>{count("todo")} to do</SummaryChip>
            <SummaryChip tone="info">{count("in_progress")} in progress</SummaryChip>
            <SummaryChip tone="warn">{count("in_review")} in review</SummaryChip>
            <SummaryChip tone="success">{count("done")} done</SummaryChip>
          </Summary>
          {open.length === 0 ? (
            <Text tone="muted" size="small">Nothing open for {project.name}.</Text>
          ) : (
            <List compact aria-label={`Open issues for ${project.name}`}>
              {open.slice(0, BLOCK_ROWS).map((i) => (
                <ListItem key={i.key}>
                  <ListButton onClick={() => openIssue(i)} aria-label={`Open ${i.key}: ${i.title}`}>
                    <StatusDot status={i.status} />
                    <TypeIcon type={i.type} />
                    <span className="tk-key">{i.key}</span>
                    <Text as="span" grow truncate>{i.title}</Text>
                    {i.status === "todo" ? null : <Text as="span" tone="muted" size="tiny">{STATUS_LABEL[i.status]}</Text>}
                    <SessionMarker issue={i} />
                  </ListButton>
                </ListItem>
              ))}
            </List>
          )}
          {open.length > BLOCK_ROWS ? <Text tone="muted" size="small">{open.length - BLOCK_ROWS} more on the board</Text> : null}
        </>
      )}
    </Stack>
  );
}
