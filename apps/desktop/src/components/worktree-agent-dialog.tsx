import { useEffect, useState, useSyncExternalStore } from "react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { api, type Space, type WorktreeMade } from "@/lib/api";
import {
  agentInput,
  baseChoices,
  clearWorktreeAgentRequest,
  suggestBranch,
  worktreeAgentStore,
  worktreeAgents,
  type WorktreeAgent,
} from "@/lib/worktree-agent";

function Form({ space, onCreated }: { space: Space; onCreated: (made: WorktreeMade, input: string | null) => void }) {
  const [branch, setBranch] = useState(suggestBranch);
  const [bases, setBases] = useState<string[]>([]);
  const [base, setBase] = useState<string | null>(null);
  const [agent, setAgent] = useState<WorktreeAgent>("claude");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let gone = false;
    api
      .worktreeInfo(space.id)
      .then((info) => {
        if (gone) return;
        const list = baseChoices(info);
        setBases(list);
        setBase(list[0] ?? null);
      })
      .catch((e) => !gone && setError(String(e)));
    return () => {
      gone = true;
    };
  }, [space.id]);

  async function create() {
    if (!base || busy) return;
    setBusy(true);
    setError(null);
    try {
      const made = await api.worktreeCreate(space.id, branch.trim(), base);
      clearWorktreeAgentRequest();
      onCreated(made, agentInput(agent));
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        void create();
      }}
    >
      <div className="grid gap-1.5">
        <Label htmlFor="worktree-branch">Branch</Label>
        <Input id="worktree-branch" value={branch} onChange={(e) => setBranch(e.target.value)} spellCheck={false} autoComplete="off" autoFocus />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="worktree-base">Start from</Label>
        <Select items={bases.map((b) => ({ value: b, label: b }))} value={base} onValueChange={(v) => v !== null && setBase(v)}>
          <SelectTrigger id="worktree-base" className="w-full">
            <SelectValue placeholder="Reading branches…" />
          </SelectTrigger>
          <SelectContent>
            {bases.map((b) => (
              <SelectItem key={b} value={b}>
                {b}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="worktree-agent">Agent</Label>
        <Select items={worktreeAgents} value={agent} onValueChange={(v) => v !== null && setAgent(v as WorktreeAgent)}>
          <SelectTrigger id="worktree-agent" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {worktreeAgents.map((a) => (
              <SelectItem key={a.value} value={a.value}>
                {a.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {error && (
        <p role="alert" className="max-h-32 overflow-y-auto rounded-lg bg-destructive/10 px-3 py-2 text-[12px] break-words whitespace-pre-wrap text-destructive">
          {error}
        </p>
      )}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={clearWorktreeAgentRequest}>
          Cancel
        </Button>
        <Button type="submit" disabled={busy || !base || branch.trim() === ""}>
          {busy && <Spinner />}
          Create worktree
        </Button>
      </DialogFooter>
    </form>
  );
}

/** Asks for a branch, a base and an agent, then makes the worktree. `onCreated` opens the tab. Opened by `requestWorktreeAgent`. */
export function WorktreeAgentDialog({ spaces, onCreated }: { spaces: Space[]; onCreated: (made: WorktreeMade, input: string | null) => void }) {
  const spaceId = useSyncExternalStore(worktreeAgentStore.subscribe, worktreeAgentStore.get);
  const space = spaces.find((s) => s.id === spaceId);
  return (
    <Dialog open={!!space} onOpenChange={(open) => !open && clearWorktreeAgentRequest()}>
      <DialogContent>
        <div className="grid gap-1">
          <DialogTitle>New agent in a worktree</DialogTitle>
          <DialogDescription>
            A new branch in its own folder beside {space?.name ?? "the project"}, so the agent can't touch your checkout.
          </DialogDescription>
        </div>
        {space && <Form key={space.id} space={space} onCreated={onCreated} />}
      </DialogContent>
    </Dialog>
  );
}
