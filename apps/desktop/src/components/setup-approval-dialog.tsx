import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, type McpSetupApproval } from "@/lib/api";

/** An agent asks to set a project's setup command. It runs in every new worktree, so nothing is saved until the user allows it. */
export function SetupApprovalDialog() {
  const [queue, setQueue] = useState<McpSetupApproval[]>([]);
  // The ids the user answered, so the timer of an answered request does nothing.
  const done = useRef(new Set<number>());

  useEffect(() => {
    const off = api.onMcpSetupApproval((ask) => {
      setQueue((q) => [...q, ask]);
      // The tool call gave up at the same time; the prompt goes with it.
      window.setTimeout(() => setQueue((q) => q.filter((a) => a.callId !== ask.callId)), ask.timeoutMs);
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  const ask = queue[0];
  // Allow waits a moment each time the request shown changes, so a click meant for one can't land on the next.
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    setArmed(false);
    if (!ask) return;
    const timer = window.setTimeout(() => setArmed(true), 1000);
    return () => window.clearTimeout(timer);
  }, [ask?.callId]);
  function answer(allow: boolean) {
    if (!ask || done.current.has(ask.callId) || (allow && !armed)) return;
    done.current.add(ask.callId);
    setQueue((q) => q.filter((a) => a.callId !== ask.callId));
    void api.mcpToolResult(ask.callId, JSON.stringify({ allow }), false);
  }

  return (
    <Dialog open={!!ask} onOpenChange={(open) => !open && answer(false)}>
      <DialogContent className="sm:max-w-lg" showCloseButton={false} data-testid="setup-approval-dialog">
        {ask && (
          <>
            <DialogHeader>
              <DialogTitle>Allow this setup command?</DialogTitle>
              <DialogDescription>
                {ask.pane} asks to set the setup command for {ask.project}.
                {queue.length > 1 && ` ${queue.length - 1} more waiting.`}
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-3 text-[13px]">
              <div className="grid gap-1">
                <span className="text-[12px] text-muted-foreground">Now</span>
                <pre className="rounded-lg bg-muted px-3 py-2 font-mono text-[12px] break-all whitespace-pre-wrap text-muted-foreground">{ask.current || "none"}</pre>
              </div>
              <div className="grid gap-1">
                <span className="text-[12px] text-muted-foreground">Proposed, exactly as it will run</span>
                <pre className="rounded-lg bg-muted px-3 py-2 font-mono text-[12px] break-all whitespace-pre-wrap" data-testid="setup-approval-command">
                  {ask.proposed || "(clear it)"}
                </pre>
              </div>
              <p className="text-[12px] text-muted-foreground">It runs in every new worktree of this project before the agent starts.</p>
            </div>
            <DialogFooter>
              <Button variant="ghost" onClick={() => answer(false)} data-testid="setup-approval-deny">
                Deny
              </Button>
              <Button onClick={() => answer(true)} disabled={!armed} data-testid="setup-approval-allow">
                Allow
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
