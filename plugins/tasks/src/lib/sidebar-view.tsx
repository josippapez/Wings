// The Issues sidebar without its data: the open issues of a project.

import { Button, Empty, Heading, List, ListButton, ListItem, Loading, Stack, Text } from "wings-plugin-ui";
import { AddIssue } from "./AddIssue";
import { openIssues, type Issue, type Op } from "./issues";
import { StatusDot } from "./parts";

export function SidebarView({ project, issues, apply, openBoard, openIssue }: { project: WingsProject; issues: Issue[] | null; apply: (op: Op) => Promise<void>; openBoard: () => void; openIssue: (issue: Issue) => void }) {
  const open = issues ? openIssues(issues) : [];
  const done = issues ? issues.length - open.length : 0;

  return (
    <Stack gap="m" className="tk-side">
      <Heading actions={<Button size="xs" variant="outline" onClick={openBoard}>Open board</Button>}>
        Issues {issues ? <span className="tk-count">{open.length}</span> : null}
      </Heading>
      <AddIssue apply={apply} />
      {!issues ? (
        <Loading>Loading issues</Loading>
      ) : open.length === 0 ? (
        <Empty title="All clear" description={done > 0 ? `Every issue for ${project.name} is closed.` : `Nothing open for ${project.name}. Add an issue above, or ask Claude to keep track of its work here.`} />
      ) : (
        <List compact aria-label={`Open issues for ${project.name}`}>
          {open.map((i) => (
            <ListItem key={i.key}>
              <ListButton onClick={() => openIssue(i)} aria-label={`Open ${i.key}: ${i.title}`}>
                <StatusDot status={i.status} />
                <span className="tk-key">{i.key}</span>
                <Text as="span" grow truncate title={i.title}>{i.title}</Text>
              </ListButton>
            </ListItem>
          ))}
        </List>
      )}
      {done > 0 && open.length > 0 ? <Text tone="muted" size="small" className="tk-done-note">{done} closed</Text> : null}
    </Stack>
  );
}
