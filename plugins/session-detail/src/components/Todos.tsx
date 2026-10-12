import type { Todo } from "../lib/types";

const boxLabels = { completed: "Done", in_progress: "In progress", pending: "To do" };

export function Todos({ todos }: { todos: Todo[] }) {
  const done = todos.filter((t) => t.status === "completed").length;
  return (
    <>
      <h2>{`To do (${done}/${todos.length})`}</h2>
      <ul className="list todos">
        {todos.map((t, i) => (
          <li key={i} className={`row todo ${t.status}`}>
            <span className={`box ${t.status}`} role="img" aria-label={boxLabels[t.status]} />
            <span className="grow">{t.status === "in_progress" && t.active ? t.active : t.text}</span>
          </li>
        ))}
      </ul>
    </>
  );
}
