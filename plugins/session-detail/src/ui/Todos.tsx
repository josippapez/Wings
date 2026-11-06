import { Dot, Heading, List, ListItem, Text } from "wings-plugin-ui";

import type { Todo, TodoStatus } from "../lib/types";

const boxLabels = { completed: "Done", in_progress: "In progress", pending: "To do" };
/** A dot that fills in as a task moves from pending to done. */
const dots: Record<TodoStatus, "muted" | "info" | undefined> = { pending: "muted", in_progress: "info", completed: undefined };

export function Todos({ todos }: { todos: Todo[] }) {
  const done = todos.filter((t) => t.status === "completed").length;
  return (
    <>
      <Heading>{`To do (${done}/${todos.length})`}</Heading>
      <List compact>
        {todos.map((t, i) => (
          <ListItem key={i} align="start" className={`todo ${t.status}`}>
            <Dot tone={dots[t.status]} label={boxLabels[t.status]} />
            <Text grow tone={t.status === "completed" ? "muted" : undefined} weight={t.status === "in_progress" ? "medium" : undefined}>
              {t.status === "in_progress" && t.active ? t.active : t.text}
            </Text>
          </ListItem>
        ))}
      </List>
    </>
  );
}
