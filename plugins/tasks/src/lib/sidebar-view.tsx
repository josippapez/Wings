// The Tasks sidebar without its data: the short list for a project.

import { Button, Checkbox, Empty, Heading, List, ListItem, Loading, Stack, Text } from "wings-plugin-ui";
import { AddTask } from "./AddTask";
import { StatusDot } from "./parts";
import { openTasks, type Op, type Task } from "./tasks";

export function SidebarView({ project, tasks, apply, board }: { project: WingsProject; tasks: Task[] | null; apply: (op: Op) => Promise<void>; board: (taskId?: string) => void }) {
  const open = tasks ? openTasks(tasks) : [];
  const done = tasks ? tasks.length - open.length : 0;

  return (
    <Stack gap="m" className="tk-side">
      <Heading actions={<Button size="xs" variant="outline" onClick={() => board()}>Open board</Button>}>
        Tasks {tasks ? <span className="tk-count">{open.length}</span> : null}
      </Heading>
      <AddTask apply={apply} branch={project.branch} />
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : open.length === 0 ? (
        <Empty title="All clear" description={done > 0 ? `Every task for ${project.name} is done.` : `Nothing open for ${project.name}. Add a task above, or ask Claude to keep a follow-up here.`} />
      ) : (
        <List compact className="tk-list" aria-label={`Open tasks for ${project.name}`}>
          {open.map((t) => (
            <ListItem key={t.id} className="tk-item">
              <Checkbox aria-label={`Mark "${t.title}" done`} checked={false} onChange={() => void apply({ kind: "update", id: t.id, status: "done" }).catch(console.error)} />
              <button type="button" className="tk-open" onClick={() => board(t.id)} aria-label={`Open task: ${t.title}`}>
                <span className="tk-item-title">{t.title}</span>
                {t.status === "doing" || t.links?.branch ? (
                  <span className="tk-item-meta">
                    {t.status === "doing" ? <span className="tk-doing"><StatusDot status="doing" /> Doing</span> : null}
                    {t.links?.branch ? <span className="tk-branch" title={t.links.branch}>{t.links.branch}</span> : null}
                  </span>
                ) : null}
              </button>
            </ListItem>
          ))}
        </List>
      )}
      {done > 0 && open.length > 0 ? <Text tone="muted" size="small" className="tk-done-note">{done} done</Text> : null}
    </Stack>
  );
}

