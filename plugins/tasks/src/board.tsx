// The Tasks tab: a board of to do, doing and done. Click a card to edit it. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons.

import { useEffect, useRef, useState } from "react";
import { Board, BoardColumn, Button, Empty, Loading, Page, Row, showTab, Stack, Summary, SummaryChip, Text, TextInput, Toolbar } from "wings-plugin-ui";
import { AddTask } from "./lib/AddTask";
import { LIMITS, STATUSES, STATUS_LABEL, type Op, type Task } from "./lib/tasks";
import { StatusDot, TaskCard } from "./lib/parts";
import { useTasks } from "./lib/useTasks";
import "./tasks.css";

const SAVE_MS = 800;

function BoardPage() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading tasks</Loading>;
  if (!project) return <Empty title="No project" description="Open a project to see its tasks." />;
  return <ProjectBoard key={project.id} project={project} />;
}

function ProjectBoard({ project }: { project: WingsProject }) {
  const { tasks, apply } = useTasks(project.id);
  const [selected, setSelected] = useState<string | null>(() => wings.viewArgs<{ taskId?: string }>()?.taskId ?? null);
  const [error, setError] = useState<string | null>(null);

  const send = (op: Op) => apply(op).then(() => setError(null), (e) => setError(e instanceof Error ? e.message : String(e)));

  // Opening the tab for a task puts it on screen and focuses it, once the list is here.
  const focused = useRef<string | null>(null);
  useEffect(() => {
    if (!tasks || !selected || focused.current === selected) return;
    const el = document.getElementById(`task-${selected}`);
    if (!el) return;
    focused.current = selected;
    el.scrollIntoView({ block: "nearest", inline: "nearest" });
    el.focus();
  }, [tasks, selected]);

  const move = (t: Task, by: 1 | -1) => {
    const next = STATUSES[STATUSES.indexOf(t.status) + by];
    if (!next) return;
    void send({ kind: "update", id: t.id, status: next }).then(() => document.getElementById(`task-${t.id}`)?.focus());
  };

  return (
    <Page
      flush
      toolbar={
        <Toolbar title={`Tasks, ${project.name}`} actions={<AddTask apply={apply} branch={project.branch} />}>
          {tasks ? (
            <Summary aria-label="Task counts">
              <SummaryChip>{tasks.filter((t) => t.status === "todo").length} to do</SummaryChip>
              <SummaryChip tone="info">{tasks.filter((t) => t.status === "doing").length} doing</SummaryChip>
              <SummaryChip tone="success">{tasks.filter((t) => t.status === "done").length} done</SummaryChip>
            </Summary>
          ) : null}
          {error ? <Text tone="danger" role="alert">{error}</Text> : null}
        </Toolbar>
      }
    >
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : (
        <Board label={`Tasks for ${project.name}`}>
          {STATUSES.map((status) => {
            const column = tasks.filter((t) => t.status === status).sort((a, b) => (status === "done" ? b.updatedAt - a.updatedAt : a.createdAt - b.createdAt));
            return (
              <BoardColumn key={status} title={<span className="tk-col-title"><StatusDot status={status} />{STATUS_LABEL[status]}</span>} count={column.length}>
                {column.length === 0 ? <p className="tk-column-empty">{status === "todo" ? "Nothing waiting" : status === "doing" ? "Nothing in progress" : "Nothing finished yet"}</p> : null}
                {column.map((t) => (
                  <TaskCard key={t.id} task={t} id={`task-${t.id}`} expanded={selected === t.id} onOpen={() => setSelected(selected === t.id ? null : t.id)} onMove={(by) => move(t, by)}>
                    {selected === t.id ? <TaskEditor key={t.id} task={t} send={send} branch={project.branch} /> : null}
                  </TaskCard>
                ))}
              </BoardColumn>
            );
          })}
        </Board>
      )}
    </Page>
  );
}

/** Title, notes and branch. They live in the inputs, not state: a keystroke costs no render, and a save goes out 800 ms after typing stops or on blur. */
function TaskEditor({ task, send, branch }: { task: Task; send: (op: Op) => Promise<void>; branch: string | null }) {
  const title = useRef<HTMLInputElement>(null);
  const notes = useRef<HTMLTextAreaElement>(null);
  const branchInput = useRef<HTMLInputElement>(null);
  const dirty = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const latest = useRef(task);
  latest.current = task;

  const save = useRef(() => {});
  save.current = () => {
    clearTimeout(timer.current);
    if (!dirty.current) return;
    dirty.current = false;
    const t = latest.current;
    const op: Op = { kind: "update", id: t.id };
    const next = { title: title.current?.value ?? t.title, notes: notes.current?.value ?? t.notes ?? "", branch: branchInput.current?.value ?? t.links?.branch ?? "" };
    if (next.title.trim() && next.title !== t.title) op.title = next.title;
    if (next.notes !== (t.notes ?? "")) op.notes = next.notes;
    if (next.branch !== (t.links?.branch ?? "")) op.branch = next.branch;
    if (op.title !== undefined || op.notes !== undefined || op.branch !== undefined) void send(op);
  };

  // Take another page's or an agent's change while nothing here is unsaved.
  useEffect(() => {
    if (dirty.current) return;
    if (title.current) title.current.value = task.title;
    if (notes.current) notes.current.value = task.notes ?? "";
    if (branchInput.current) branchInput.current.value = task.links?.branch ?? "";
  }, [task.title, task.notes, task.links?.branch]);

  useEffect(() => {
    const flush = () => save.current();
    window.addEventListener("pagehide", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, []);

  const edited = () => {
    dirty.current = true;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => save.current(), SAVE_MS);
  };

  return (
    <Stack gap="s" className="tk-editor">
      <TextInput ref={title} label="Title" defaultValue={task.title} maxLength={LIMITS.title} onInput={edited} onBlur={() => save.current()} />
      <textarea ref={notes} aria-label="Notes" placeholder="Notes" rows={5} maxLength={LIMITS.notes} defaultValue={task.notes ?? ""} onInput={edited} onBlur={() => save.current()} />
      <TextInput ref={branchInput} label="Branch" placeholder="No branch" defaultValue={task.links?.branch ?? ""} maxLength={LIMITS.branch} onInput={edited} onBlur={() => save.current()} />
      <Row gap="s" wrap>
        {branch && branch !== task.links?.branch ? (
          <Button size="xs" variant="outline" onClick={() => { if (branchInput.current) branchInput.current.value = branch; edited(); save.current(); }}>
            Use branch {branch}
          </Button>
        ) : null}
        <Button size="xs" variant="destructive" onClick={() => { dirty.current = false; void send({ kind: "remove", id: task.id }); }}>Delete</Button>
      </Row>
    </Stack>
  );
}

showTab(<BoardPage />, { fill: true });
