// A page's view of one project's tasks: read from storage, read again whenever main says they changed, and changes
// go to main as operations.

import { useCallback, useEffect, useRef, useState } from "react";
import { readTasks, type Op, type OpMessage, type Task, type TasksMessage } from "./tasks";

export function useTasks(projectId: string): { tasks: Task[] | null; apply: (op: Op) => Promise<void> } {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const pending = useRef(new Map<string, { resolve: () => void; reject: (error: Error) => void }>());
  const count = useRef(0);

  useEffect(() => {
    let live = true;
    const load = () => void readTasks(projectId).then((list) => live && setTasks(list));
    load();
    const off = wings.onBroadcast((message) => {
      const m = message as TasksMessage | null;
      if (m?.type !== "tasks" || m.projectId !== projectId) return;
      const waiting = m.rid ? pending.current.get(m.rid) : undefined;
      if (waiting && m.rid) {
        pending.current.delete(m.rid);
        if (m.error) waiting.reject(new Error(m.error));
        else waiting.resolve();
      }
      load();
    });
    return () => {
      live = false;
      off();
    };
  }, [projectId]);

  const apply = useCallback(
    (op: Op) =>
      new Promise<void>((resolve, reject) => {
        const rid = `${Date.now().toString(36)}-${++count.current}`;
        pending.current.set(rid, { resolve, reject });
        wings.broadcast({ type: "op", projectId, rid, op } satisfies OpMessage).catch((error) => {
          pending.current.delete(rid);
          reject(error instanceof Error ? error : new Error(String(error)));
        });
      }),
    [projectId],
  );

  return { tasks, apply };
}
