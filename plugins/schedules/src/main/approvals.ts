// An agent asks to add or change a timed schedule: the "Allow this schedule?" modal shows it, and the tool call waits for
// the user's answer. Asks live only while their call waits, so one nobody answers is gone with it.

import type { Approval } from "../lib/approval";
import { ch } from "../lib/messages";

/** Under the 120 s Wings gives a plugin's tool call, so the call can still say nobody answered. */
const WAIT_MS = 100_000;
const VIEW = "approve";

export type Answer = "allow" | "deny" | "timeout";

const pending = new Map<string, { approval: Approval; settle: (answer: Answer) => void }>();
let next = 0;

export function serveApprovals() {
  ch.serve("approvals", () => [...pending.values()].map((p) => p.approval));
  ch.answer("answerApproval", ({ id, allow }) => {
    const ask = pending.get(id);
    if (!ask) throw new Error("That request is gone: the agent stopped waiting.");
    ask.settle(allow ? "allow" : "deny");
  });
}

/** Shows the ask in the modal, and resolves the user's answer, or "timeout". */
export function askApproval(ask: Omit<Approval, "id">, projectId: string): Promise<Answer> {
  const id = String(++next);
  return new Promise((resolve) => {
    const timer = setTimeout(() => settle("timeout"), WAIT_MS);
    function settle(answer: Answer) {
      if (!pending.delete(id)) return;
      clearTimeout(timer);
      ch.refresh("approvals");
      if (!pending.size) wings.closeView(VIEW).catch(console.error);
      resolve(answer);
    }
    pending.set(id, { approval: { ...ask, id }, settle });
    ch.refresh("approvals");
    wings.openView(VIEW, { as: "modal" }).catch(console.error);
    if (!wings.focused) {
      wings.notify({ title: ask.current ? "An agent asks to change a schedule" : "An agent asks to add a schedule", body: `In ${ask.projectName}. Allow or deny it in Wings.`, projectId }).catch(console.error);
    }
  });
}
