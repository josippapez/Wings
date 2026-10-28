// ProjectGate for Issues pages: a page in a worktree Wings made shows its main project's issues, so `project` has the
// main project's id and name (its path stays the worktree's, where terminals open).

import { useEffect, useState, type ReactNode } from "react";
import { Loading, ProjectGate } from "wings-plugin-ui";
import { homeOf } from "../lib/home";

type Props = { loading?: ReactNode; empty?: { title?: ReactNode; description?: ReactNode }; children: (project: WingsProject) => ReactNode };

export function HomeGate({ loading, empty, children }: Props) {
  return (
    <ProjectGate loading={loading} empty={empty}>
      {(project) => <Home project={project} loading={loading}>{children}</Home>}
    </ProjectGate>
  );
}

function Home({ project, loading, children }: { project: WingsProject; loading?: ReactNode; children: Props["children"] }) {
  const [home, setHome] = useState<{ id: string; name: string } | null>(null);
  useEffect(() => void homeOf(project).then(setHome), [project.id, project.name, project.parentId]);
  return home ? <>{children({ ...project, ...home })}</> : <Loading>{loading}</Loading>;
}
