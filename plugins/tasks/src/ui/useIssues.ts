// A page's view of one project's issues: read from storage, and read again whenever main says they changed. Changes go to
// main as operations.

import { useCallback, useEffect, useState } from "react";
import type { Op, Store } from "../lib/domain";
import { ch } from "../lib/messages";
import { readStore } from "../lib/storage";

export function useIssues(projectId: string, projectName: string): { store: Store | null; apply: (op: Op) => Promise<void> } {
  const [store, setStore] = useState<Store | null>(null);

  useEffect(() => {
    let live = true;
    const load = () => void readStore(projectId, projectName).then((s) => live && setStore(s));
    load();
    const off = ch.on("issues", (m) => m.projectId === projectId && load());
    return () => {
      live = false;
      off();
    };
  }, [projectId, projectName]);

  const apply = useCallback((op: Op) => ch.ask("op", { projectId, projectName, op }), [projectId, projectName]);

  return { store, apply };
}
