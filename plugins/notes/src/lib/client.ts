// A page's side: read the info of a project, keep it current, and ask main for changes.

import { useEffect, useState } from "react";
import type { Info, Op } from "./model";
import type { InfoMessage, OpMessage } from "./messages";
import { readInfo } from "./store";

/** The project this page is about, `undefined` until known, `null` when there is none. */
export function useProject() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  return project;
}

/** A project's info, read again each time main says it changed. `info` is `undefined` until the first read, and `error` says when what is stored can't be read. */
export function useInfo(projectId: string): { info: Info | null | undefined; error: string } {
  const [state, setState] = useState<{ info: Info | null | undefined; error: string }>({ info: undefined, error: "" });
  useEffect(() => {
    let alive = true;
    const read = () =>
      void readInfo(projectId).then(
        (info) => alive && setState({ info, error: "" }),
        (e) => alive && setState({ info: null, error: e instanceof Error ? e.message : String(e) }),
      );
    read();
    const off = wings.onBroadcast((message) => {
      const m = message as InfoMessage | null;
      if (m?.type === "info" && m.projectId === projectId) read();
    });
    return () => {
      alive = false;
      off();
    };
  }, [projectId]);
  return state;
}

/** How long a page waits for main to answer. Main may not be running yet, or may have reloaded. */
const TIMEOUT_MS = 15_000;

/** Asks main to apply `op`; resolves when main has saved it, rejects with its message, or with a `Timeout` error. */
export function request(projectId: string, op: Op): Promise<void> {
  const rid = crypto.randomUUID();
  return new Promise((resolve, reject) => {
    const done = (finish: () => void) => {
      off();
      clearTimeout(timer);
      finish();
    };
    const off = wings.onBroadcast((message) => {
      const m = message as InfoMessage | null;
      if (m?.type !== "info" || m.rid !== rid) return;
      done(() => (m.error ? reject(new Error(m.error)) : resolve()));
    });
    const timer = setTimeout(
      () => done(() => reject(Object.assign(new Error("Wings didn't answer in time. Your text is still here; it will be sent again when you type."), { name: "Timeout" }))),
      TIMEOUT_MS,
    );
    wings.broadcast({ type: "op", projectId, rid, op } satisfies OpMessage).catch((error) => done(() => reject(error)));
  });
}
