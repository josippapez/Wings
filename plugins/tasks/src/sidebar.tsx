// The Tasks sidebar: the short list for the project on screen, to add a task and tick one off.

import { useEffect, useState } from "react";
import { Button, Checkbox, Empty, Heading, List, ListItem, Loading, Row, showSidebar, Stack, Text } from "wings-plugin-ui";
import { AddTask } from "./lib/AddTask";
import { openTasks } from "./lib/tasks";
import { useTasks } from "./lib/useTasks";

function Sidebar() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading tasks</Loading>;
  if (!project) return <Empty title="No project on screen" description="Open a project to see its tasks." />;
  return <ProjectTasks key={project.id} project={project} />;
}

function ProjectTasks({ project }: { project: WingsProject }) {
  const { tasks, apply } = useTasks(project.id);
  const open = tasks ? openTasks(tasks) : [];
  const done = tasks ? tasks.length - open.length : 0;
  const board = (taskId?: string) => void wings.openView("board", { as: "tab", args: taskId ? { taskId } : undefined });

  return (
    <Stack gap="m">
      <Heading actions={<Button size="xs" variant="outline" onClick={() => board()}>Open board</Button>}>Tasks</Heading>
      <AddTask apply={apply} branch={project.branch} />
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : open.length === 0 ? (
        <Text tone="muted">Nothing open for {project.name}.</Text>
      ) : (
        <List compact aria-label={`Open tasks for ${project.name}`}>
          {open.map((t) => (
            <ListItem key={t.id}>
              <Row gap="s">
                <Checkbox aria-label={`Mark "${t.title}" done`} checked={false} onChange={() => void apply({ kind: "update", id: t.id, status: "done" }).catch(console.error)} />
                <Button variant="ghost" size="sm" onClick={() => board(t.id)}>
                  {t.status === "doing" ? <Text tone="info" size="tiny">Doing</Text> : null} {t.title}
                </Button>
              </Row>
            </ListItem>
          ))}
        </List>
      )}
      {done > 0 ? <Text tone="muted" size="small">{done} done</Text> : null}
    </Stack>
  );
}

showSidebar(<Sidebar />);
