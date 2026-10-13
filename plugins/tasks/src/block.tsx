// The Board block of a Project page: the project's tasks as to do, doing and done, compact. Add a task on the
// line above, move one with its buttons or Alt+Left and Alt+Right, open the board tab for the rest.

import { useEffect, useRef, useState } from "react";
import { Board, BoardColumn, Chip, Empty, Loading, Row, showView, Stack, Text } from "wings-plugin-ui";
import { AddTask } from "./lib/AddTask";
import { boardColumns, cleanBlockState, STATUSES, STATUS_LABEL, type BoardBlockState, type Op, type Task } from "./lib/tasks";
import { TaskCard, StatusDot } from "./lib/parts";
import { useTasks } from "./lib/useTasks";
import "./tasks.css";

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
  else if (!project) body = <Empty title="No project" description="Open a project to see its tasks." />;
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
      <Row gap="m" justify="between">
        <AddTask apply={apply} branch={project.branch} />
        <Chip pressed={hideDone} onClick={() => onHideDone(!hideDone)}>Hide done</Chip>
      </Row>
      {error ? <Text tone="danger" role="alert" size="small">{error}</Text> : null}
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : tasks.length === 0 ? (
        <Empty title="No tasks yet" description="Add one above." />
      ) : (
        <Board className="tasks-block-board" label={`Tasks for ${project.name}`}>
          {boardColumns(tasks, hideDone).map(({ status, tasks: column }) => (
            <BoardColumn key={status} title={<span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>} count={column.length}>
              {column.length === 0 ? <p className="tk-column-empty">Nothing here</p> : null}
              {column.map((t) => <TaskCard key={t.id} task={t} id={`block-task-${t.id}`} compact onMove={(by) => move(t, by)} onOpen={() => void wings.openView("board", { as: "tab", args: { taskId: t.id } })} />)}
            </BoardColumn>
          ))}
        </Board>
      )}
    </Stack>
  );
}

showView(<BlockPage />);
