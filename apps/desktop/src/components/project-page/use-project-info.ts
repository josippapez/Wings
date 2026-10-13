import { useCallback, useEffect, useRef, useState } from "react";

import { projectInfo, type Info, type Op } from "@/lib/project-info";

export const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * A project's page content, kept current: it reads on mount and again when something else (an agent through MCP)
 * changes it. Reads never overlap, a read that arrives after a newer answer is dropped, and `apply` shows the answer to
 * your own change at once. `onApplied` runs with the new and the previous info just before the page shows the new one.
 */
export function useProjectInfo(projectId: string) {
  const [info, setInfo] = useState<Info | undefined>(undefined);
  const [error, setError] = useState("");
  const infoRef = useRef<Info | undefined>(undefined);
  const ticket = useRef(0);
  const landed = useRef(0);
  const loading = useRef(false);
  const again = useRef(false);

  const take = useCallback((t: number, next: Info) => {
    if (t < landed.current) return;
    landed.current = t;
    infoRef.current = next;
    setInfo(next);
  }, []);

  const load = useCallback(() => {
    // One read at a time; changes that arrive meanwhile cost one more read after it.
    if (loading.current) {
      again.current = true;
      return;
    }
    loading.current = true;
    const t = ++ticket.current;
    projectInfo
      .get(projectId)
      .then(
        (next) => {
          take(t, next);
          setError("");
        },
        (e) => setError(messageOf(e)),
      )
      .finally(() => {
        loading.current = false;
        if (again.current) {
          again.current = false;
          load();
        }
      });
  }, [projectId, take]);

  useEffect(() => {
    setInfo(undefined);
    infoRef.current = undefined;
    landed.current = ticket.current;
    load();
    // `onChanged` answers with a promise of the unsubscribe.
    const off = projectInfo.onChanged((id) => id === projectId && load());
    return () => void off.then((unlisten) => unlisten());
  }, [projectId, load]);

  const apply = useCallback(
    async (op: Op, onApplied?: (next: Info, before: Info | undefined) => void) => {
      const t = ++ticket.current;
      const next = await projectInfo.apply(projectId, op);
      onApplied?.(next, infoRef.current);
      take(t, next);
      return next;
    },
    [projectId, take],
  );

  return { info, error, apply };
}

export type Apply = ReturnType<typeof useProjectInfo>["apply"];
