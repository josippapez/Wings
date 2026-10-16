// The Todo tab: sessions with their steps and history, none yet, while they load, and as the page runs it.
import TodoPage, { page } from "../src/pages/todo";
import { slotOf } from "../src/lib/storage";
import { TodoView } from "../src/ui/todo-view";
import { issues, project, storage, todos } from "./_data";

export { page };

const noop = () => {};
const view = { project, issues, onOpenIssue: noop };

export const states = {
  lists: { element: <TodoView {...view} lists={todos} /> },
  empty: { element: <TodoView {...view} lists={[]} /> },
  loading: { element: <TodoView {...view} lists={null} /> },
  // As the page runs it, with its lists read from storage.
  page: {
    element: <TodoPage />,
    project,
    storage: { ...storage, ...Object.fromEntries(todos.map((t) => [`todo/${slotOf(project.id)}/${t.sessionId}`, t])) },
  },
};
