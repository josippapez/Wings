// The Tasks tab: a board of to do, doing and done. Click a card to edit it. Alt+Left and Alt+Right on a card move it
// between columns, and so do its buttons.

import { useEffect, useRef, useState } from "react";
import { Board, BoardColumn, Button, Card, Empty, Loading, Page, Row, showTab, Stack, Text, TextInput, Toolbar } from "wings-plugin-ui";
import { AddTask } from "./lib/AddTask";
import { LIMITS, STATUSES, STATUS_LABEL, type Op, type Task } from "./lib/tasks";
import { useTasks } from "./lib/useTasks";

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
      toolbar={<Toolbar title={`Tasks, ${project.name}`} actions={<AddTask apply={apply} branch={project.branch} />}>{error ? <Text tone="danger" role="alert">{error}</Text> : null}</Toolbar>}
    >
      {!tasks ? (
        <Loading>Loading tasks</Loading>
      ) : (
        <Board label={`Tasks for ${project.name}`}>
          {STATUSES.map((status) => {
            const column = tasks.filter((t) => t.status === status).sort((a, b) => (status === "done" ? b.updatedAt - a.updatedAt : a.createdAt - b.createdAt));
            return (
              <BoardColumn key={status} title={STATUS_LABEL[status]} count={column.length}>
                {column.map((t) => (
                  <TaskCard key={t.id} task={t} open={selected === t.id} onToggle={() => setSelected(selected === t.id ? null : t.id)} onMove={(by) => move(t, by)} send={send} branch={project.branch} />
                ))}
              </BoardColumn>
            );
          })}
        </Board>
      )}
    </Page>
  );
}

function TaskCard({ task, open, onToggle, onMove, send, branch }: { task: Task; open: boolean; onToggle: () => void; onMove: (by: 1 | -1) => void; send: (op: Op) => Promise<void>; branch: string | null }) {
  const index = STATUSES.indexOf(task.status);
  return (
    <Card>
      <Stack gap="s">
        <Button
          id={`task-${task.id}`}
          variant="ghost"
          aria-expanded={open}
          onClick={onToggle}
          onKeyDown={(e) => {
            if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
            e.preventDefault();
            onMove(e.key === "ArrowRight" ? 1 : -1);
          }}
        >
          <Text as="span" weight="medium" clamp>{task.title}</Text>
        </Button>
        {task.links?.branch ? <Text tone="muted" size="tiny" mono truncate>{task.links.branch}</Text> : null}
        {open ? <TaskEditor key={task.id} task={task} send={send} branch={branch} /> : null}
        <Row gap="s" justify="between">
          <Button size="xs" variant="outline" disabled={index === 0} aria-label={`Move "${task.title}" to ${STATUS_LABEL[STATUSES[index - 1] ?? task.status]}`} onClick={() => onMove(-1)}>←</Button>
          <Button size="xs" variant="outline" disabled={index === STATUSES.length - 1} aria-label={`Move "${task.title}" to ${STATUS_LABEL[STATUSES[index + 1] ?? task.status]}`} onClick={() => onMove(1)}>→</Button>
        </Row>
      </Stack>
    </Card>
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
    <Stack gap="s">
      <TextInput ref={title} label="Title" defaultValue={task.title} maxLength={LIMITS.title} onInput={edited} onBlur={() => save.current()} />
      <textarea ref={notes} aria-label="Notes" placeholder="Notes" rows={5} maxLength={LIMITS.notes} defaultValue={task.notes ?? ""} onInput={edited} onBlur={() => save.current()} />
      <TextInput ref={branchInput} label="Branch" placeholder="No branch" defaultValue={task.links?.branch ?? ""} maxLength={LIMITS.branch} onInput={edited} onBlur={() => save.current()} />
      <Row gap="s">
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
