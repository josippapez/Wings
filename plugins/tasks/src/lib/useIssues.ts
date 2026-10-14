// A page's view of one project's issues: read from storage, read again whenever main says they changed, and changes
// go to main as operations.

import { useCallback, useEffect, useRef, useState } from "react";
import { readStore, type IssuesMessage, type Op, type OpMessage, type Store } from "./issues";

export function useIssues(projectId: string, projectName: string): { store: Store | null; apply: (op: Op) => Promise<void> } {
  const [store, setStore] = useState<Store | null>(null);
  const pending = useRef(new Map<string, { resolve: () => void; reject: (error: Error) => void }>());
  const count = useRef(0);

  useEffect(() => {
    let live = true;
    const load = () => void readStore(projectId, projectName).then((s) => live && setStore(s));
    load();
    const off = wings.onBroadcast((message) => {
      const m = message as IssuesMessage | null;
      if (m?.type !== "issues" || m.projectId !== projectId) return;
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
  }, [projectId, projectName]);

  const apply = useCallback(
    (op: Op) =>
      new Promise<void>((resolve, reject) => {
        const rid = `${Date.now().toString(36)}-${++count.current}`;
        pending.current.set(rid, { resolve, reject });
        wings.broadcast({ type: "op", projectId, projectName, rid, op } satisfies OpMessage).catch((error) => {
          pending.current.delete(rid);
          reject(error instanceof Error ? error : new Error(String(error)));
        });
      }),
    [projectId, projectName],
  );

  return { store, apply };
}
