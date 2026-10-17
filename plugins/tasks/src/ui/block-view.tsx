// The Issues block of a Project page without its data: counts and a compact list of the open issues.

import { Button, Empty, List, ListButton, ListItem, Loading, Row, Stack, Text } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { STATUS_LABEL, type Issue, type Op } from "../lib/domain";
import { openIssues } from "../lib/orderings";
import { SessionMarker, StatusDot, StatusSummary, TypeIcon } from "./parts";

/** The block lists this many open issues; the board has the rest. */
export const BLOCK_ROWS = 8;

export interface BlockViewProps {
  project: WingsProject;
  issues: Issue[] | null;
  apply: (op: Op) => Promise<void>;
  openBoard: () => void;
  openIssue: (issue: Issue) => void;
}

export function BlockView({ project, issues, apply, openBoard, openIssue }: BlockViewProps) {
  const open = issues ? openIssues(issues) : [];
  return (
    <Stack gap="s">
      <Row gap="m" justify="between" wrap>
        <AddIssue apply={apply} />
        <Button variant="outline" size="sm" onClick={openBoard}>Open board</Button>
      </Row>
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : issues.length === 0 ? (
        <Empty title="No issues yet" description="Add one above, or ask Claude to keep track of its work here." />
      ) : (
        <>
          <StatusSummary issues={issues} />
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
          {open.length > BLOCK_ROWS ? <Button variant="link" size="xs" onClick={openBoard}>{open.length - BLOCK_ROWS} more on the board</Button> : null}
        </>
      )}
    </Stack>
  );
}
