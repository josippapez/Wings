// The Board block without its data: the compact board for a list of tasks.

import { Board, BoardColumn, Chip, Empty, Loading, Row, Stack, Text } from "wings-plugin-ui";
import { AddTask } from "./AddTask";
import { boardColumns, STATUS_LABEL, type Op, type Task } from "./tasks";
import { StatusDot, TaskCard } from "./parts";

export interface BlockViewProps {
  project: WingsProject;
  tasks: Task[] | null;
  error: string | null;
  hideDone: boolean;
  onHideDone: (hide: boolean) => void;
  apply: (op: Op) => Promise<void>;
  onMove: (t: Task, by: 1 | -1) => void;
  onOpen: (t: Task) => void;
}

export function BlockView({ project, tasks, error, hideDone, onHideDone, apply, onMove, onOpen }: BlockViewProps) {
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
              {column.map((t) => <TaskCard key={t.id} task={t} id={`block-task-${t.id}`} compact onMove={(by) => onMove(t, by)} onOpen={() => onOpen(t)} />)}
            </BoardColumn>
          ))}
        </Board>
      )}
    </Stack>
  );
}

