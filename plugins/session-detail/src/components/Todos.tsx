import { Heading, List, ListItem, Text } from "wings-plugin-ui";

import type { Todo } from "../lib/types";

const boxLabels = { completed: "Done", in_progress: "In progress", pending: "To do" };

export function Todos({ todos }: { todos: Todo[] }) {
  const done = todos.filter((t) => t.status === "completed").length;
  return (
    <>
      <Heading>{`To do (${done}/${todos.length})`}</Heading>
      <List compact>
        {todos.map((t, i) => (
          <ListItem key={i} align="start" className={`todo ${t.status}`}>
            <span className={`box ${t.status}`} role="img" aria-label={boxLabels[t.status]} />
            <Text grow>{t.status === "in_progress" && t.active ? t.active : t.text}</Text>
          </ListItem>
        ))}
      </List>
    </>
  );
}
