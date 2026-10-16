// A page's view of one project's todo lists: read from storage, and read again whenever main says they changed.

import { useEffect, useState } from "react";
import { ch } from "../lib/messages";
import { readTodos } from "../lib/todo-store";
import type { TodoList } from "../lib/todo";

export function useTodos(projectId: string): TodoList[] | null {
  const [lists, setLists] = useState<TodoList[] | null>(null);
  useEffect(() => {
    let live = true;
    const load = () => void readTodos(projectId).then((l) => live && setLists(l));
    load();
    const off = ch.on("todo", (m) => m.projectId === projectId && load());
    return () => {
      live = false;
      off();
    };
  }, [projectId]);
  return lists;
}
