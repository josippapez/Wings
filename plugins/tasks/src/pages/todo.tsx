// The Todo tab: the agent sessions of this project that keep a step list, with their steps and history. Read-only.

import { HomeGate } from "../ui/HomeGate";
import { TodoView } from "../ui/todo-view";
import { useIssues } from "../ui/useIssues";
import { useTodos } from "../ui/useTodos";
import "./tasks.css";

export const page = { place: "tab", fill: true };

export default function TodoPage() {
  return (
    <HomeGate loading="Loading todo lists" empty={{ description: "Open a project to see its todo lists." }}>
      {(project) => <ProjectTodo project={project} />}
    </HomeGate>
  );
}

function ProjectTodo({ project }: { project: WingsProject }) {
  const lists = useTodos(project.id);
  const { store } = useIssues(project.id, project.name);
  const open = (key: string, newTab: boolean) => void wings.openView("issue", { as: "tab", args: { key, projectId: project.id }, newTab });
  return <TodoView project={project} lists={lists} issues={store?.issues ?? []} onOpenIssue={open} />;
}
