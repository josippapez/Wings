// The Board block of a Project page: the project's tasks as to do, doing and done, compact. Add a task on the
// line above, move one with its buttons or Alt+Left and Alt+Right, open the board tab for the rest.

import { useEffect, useRef, useState } from "react";
import { Empty, Loading, showView } from "wings-plugin-ui";
import { BlockView } from "./lib/block-view";
import { cleanBlockState, STATUSES, type BoardBlockState, type Op, type Task } from "./lib/tasks";
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

  return <BlockView project={project} tasks={tasks} error={error} hideDone={hideDone} onHideDone={onHideDone} apply={apply} onMove={move} onOpen={(t) => void wings.openView("board", { as: "tab", args: { taskId: t.id } })} />;
}

showView(<BlockPage />);
