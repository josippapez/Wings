// The Board block of a Project page: the project's tasks as to do, doing and done, compact. Add a task on the
// line above, move one with its buttons or Alt+Left and Alt+Right, open the board tab for the rest.

import { useEffect, useRef, useState } from "react";
import { Board, BoardColumn, Button, Card, Chip, Empty, Loading, Row, showView, Stack, Text } from "wings-plugin-ui";
import { AddTask } from "./lib/AddTask";
import { boardColumns, cleanBlockState, STATUSES, STATUS_LABEL, type BoardBlockState, type Op, type Task } from "./lib/tasks";
import { useTasks } from "./lib/useTasks";
import "./block.css";

/** Tells Wings how tall the content is, so the block fits it. */
function useFit(ref: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = new ResizeObserver(() => void wings.setHeight(Math.ceil(el.getBoundingClientRect().height)));
    fit.observe(el);
    return () => fit.disconnect();
  }, [ref]);
}

function BlockPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  const [state, setState] = useState<BoardBlockState | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    void wings.project().then(setProject);
    void Promise.resolve(wings.blockState()).then((raw) => setState(cleanBlockState(raw)));
  }, []);
  useFit(root);

  const setHideDone = (hideDone: boolean) => {
    const next = hideDone ? { hideDone } : {};
    setState(next);
    void wings.setBlockState(next);
  };

  let body;
  if (project === undefined || !state) body = <Loading>Loading tasks</Loading>;
  else if (!project) body = <Text tone="muted">No project.</Text>;
  else body = <ProjectBlock key={project.id} project={project} hideDone={state.hideDone === true} onHideDone={setHideDone} />;
  return <div ref={root}>{body}</div>;
}

function ProjectBlock({ project, hideDone, onHideDone }: { project: WingsProject; hideDone: boolean; onHideDone: (hide: boolean) => void }) {
  const { tasks, apply } = useTasks(project.id);
  const [error, setError] = useState<string | null>(null);

  const move = (t: Task, by: 1 | -1) => {
    const next = STATUSES[STATUSES.indexOf(t.status) + by];
    if (!next) return;
    apply({ kind: "update", id: t.id, status: next } satisfies Op).then(
      () => {
        setError(null);
        document.getElementById(`block-task-${t.id}`)?.focus();
      },
      (e) => setError(e instanceof Error ? e.message : String(e)),
    );
  };

  return (
    <Stack gap="s">
      <Row gap="s" justify="between">
        <AddTask apply={apply} branch={project.branch} />
        <Chip pressed={hideDone} onClick={() => onHideDone(!hideDone)}>Hide done</Chip>
      </Row>
      {error ? <Text tone="danger" role="alert">{error}</Text> : null}
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : tasks.length === 0 ? (
        <Empty title="No tasks yet" description="Add one above." />
      ) : (
        <Board className="tasks-block-board" label={`Tasks for ${project.name}`}>
          {boardColumns(tasks, hideDone).map(({ status, tasks: column }) => (
            <BoardColumn key={status} title={STATUS_LABEL[status]} count={column.length}>
              {column.map((t) => <TaskCard key={t.id} task={t} onMove={(by) => move(t, by)} onOpen={() => void wings.openView("board", { as: "tab", args: { taskId: t.id } })} />)}
            </BoardColumn>
          ))}
        </Board>
      )}
    </Stack>
  );
}

function TaskCard({ task, onMove, onOpen }: { task: Task; onMove: (by: 1 | -1) => void; onOpen: () => void }) {
  const index = STATUSES.indexOf(task.status);
  return (
    <Card compact>
      <Stack gap="xs">
        <Button
          id={`block-task-${task.id}`}
          variant="ghost"
          onClick={onOpen}
          onKeyDown={(e) => {
            if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
            e.preventDefault();
            onMove(e.key === "ArrowRight" ? 1 : -1);
          }}
        >
          <Text as="span" weight="medium" clamp>{task.title}</Text>
        </Button>
        <Row gap="s" justify="between">
          <Button size="xs" variant="outline" disabled={index === 0} aria-label={`Move "${task.title}" to ${STATUS_LABEL[STATUSES[index - 1] ?? task.status]}`} onClick={() => onMove(-1)}>←</Button>
          <Button size="xs" variant="outline" disabled={index === STATUSES.length - 1} aria-label={`Move "${task.title}" to ${STATUS_LABEL[STATUSES[index + 1] ?? task.status]}`} onClick={() => onMove(1)}>→</Button>
        </Row>
      </Stack>
    </Card>
  );
}

showView(<BlockPage />);
