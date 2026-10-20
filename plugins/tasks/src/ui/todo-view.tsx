// The Todo tab without its data: the project's agent sessions that keep a step list, newest first, each with its current steps
// and the earlier versions behind a disclosure. The tab page feeds it from storage, a fixture from made-up data.

import { Badge, Button, Card, Disclosure, Empty, Loading, Meter, Page, Row, Stack, Text, Time, Title, Toolbar } from "wings-plugin-ui";
import { AGENTS, kindOf } from "../lib/commands";
import { iso } from "../lib/format";
import { counts, itemsOf, type TodoList } from "../lib/todo";
import type { Issue } from "../lib/domain";
import { openProps } from "./parts";
import { TodoItems } from "./todo-items";

export interface TodoViewProps {
  project: WingsProject;
  /** `null` while they load. */
  lists: TodoList[] | null;
  issues: Issue[];
  onOpenIssue: (key: string, newTab: boolean) => void;
}

function Session({ list, issue, onOpenIssue }: { list: TodoList; issue?: Issue; onOpenIssue: TodoViewProps["onOpenIssue"] }) {
  const items = itemsOf(list);
  const { done, total } = counts(items);
  const earlier = list.versions.slice(0, -1).reverse();
  const name = list.sessionName || `${AGENTS[kindOf({ kind: list.sessionKind })].label} session`;
  return (
    <Card as="section" aria-label={`Todo of ${name}`} className="tk-todo-card">
      <Row gap="m" justify="between" wrap>
        <Row gap="s" wrap>
          <Title as="h2" truncate>{name}</Title>
          {list.issueKey ? (
            <Button variant="outline" size="xs" title={issue?.title ?? list.issueKey} aria-label={`Open ${list.issueKey}${issue ? `: ${issue.title}` : ""}`} {...openProps((newTab) => onOpenIssue(list.issueKey!, newTab))}>{list.issueKey}</Button>
          ) : null}
        </Row>
        <Text as="span" tone="muted" size="small"><Time iso={iso(list.updatedAt)} prefix="updated " /></Text>
      </Row>
      <Row gap="s">
        <Meter percent={total ? (done / total) * 100 : 0} label="Steps done" tone="success" className="tk-todo-meter" />
        <Text as="span" tone="muted" size="small" className="tk-todo-count">{done} of {total}</Text>
      </Row>
      <TodoItems items={items} label={`Steps of ${name}`} />
      {earlier.length ? (
        <Disclosure summary={<Text as="span" size="small" tone="muted">History · {earlier.length} earlier {earlier.length === 1 ? "version" : "versions"}</Text>}>
          <Stack gap="m" className="tk-todo-history">
            {earlier.map((v) => (
              <Stack key={v.at} gap="s">
                <Row gap="s">
                  <Badge variant="outline"><Time iso={iso(v.at)} /></Badge>
                  <Text as="span" tone="muted" size="tiny">{counts(v.items).done} of {v.items.length} done</Text>
                </Row>
                <TodoItems items={v.items} label={`Steps as of ${iso(v.at)}`} />
              </Stack>
            ))}
          </Stack>
        </Disclosure>
      ) : null}
    </Card>
  );
}

export function TodoView({ project, lists, issues, onOpenIssue }: TodoViewProps) {
  const toolbar = <Toolbar title="Todo" />;
  if (!lists) return <Page toolbar={toolbar}><Loading>Loading todo lists</Loading></Page>;
  if (lists.length === 0) return <Page toolbar={toolbar}><Empty title="No todo lists yet" description={`When Claude keeps its steps with tasks__todo in ${project.name}, they show here.`} /></Page>;
  return (
    <Page toolbar={toolbar}>
      <Stack gap="m" className="tk-todo-wrap">
        {lists.map((l) => <Session key={l.sessionId} list={l} issue={issues.find((i) => i.key === l.issueKey)} onOpenIssue={onOpenIssue} />)}
      </Stack>
    </Page>
  );
}
