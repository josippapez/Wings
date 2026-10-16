// An agent's steps as a read-only list, shared by the Todo tab and the issue page.

import { Text } from "wings-plugin-ui";
import type { TodoItem, TodoStatus } from "../lib/todo";

const MARK: Record<TodoStatus, { glyph: string; label: string }> = {
  done: { glyph: "✓", label: "Done" },
  in_progress: { glyph: "●", label: "In progress" },
  pending: { glyph: "○", label: "Not started" },
};

export function TodoItems({ items, label }: { items: TodoItem[]; label: string }) {
  return (
    <ul className="tk-todo-items" aria-label={label}>
      {items.map((item, i) => (
        <li key={i} className={`tk-todo-item tk-todo-${item.status}`}>
          <span className="tk-todo-mark" role="img" aria-label={MARK[item.status].label}>{MARK[item.status].glyph}</span>
          <Text as="span" size="small" tone={item.status === "done" ? "muted" : undefined}>{item.text}</Text>
        </li>
      ))}
    </ul>
  );
}
