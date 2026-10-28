import { useEffect, useState, useSyncExternalStore } from "react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { api, type Space, type WorktreeLocal } from "@/lib/api";
import { requestWorktreeSetup, worktreeSetupStore } from "@/lib/worktree-agent";

function Form({ space }: { space: Space }) {
  const [local, setLocal] = useState<WorktreeLocal | null>(null);
  const [include, setInclude] = useState("");
  const [setup, setSetup] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let gone = false;
    api
      .worktreeLocal(space.id)
      .then((got) => {
        if (gone) return;
        setLocal(got);
        setInclude(got.include ?? got.defaultInclude);
        setSetup(got.setup ?? "");
      })
      .catch((e) => !gone && setError(String(e)));
    return () => {
      gone = true;
    };
  }, [space.id]);

  async function save() {
    if (!local || busy) return;
    setBusy(true);
    setError(null);
    try {
      // The default list left as it is stays unset, so a later change to the default reaches this project.
      const own = include.trim() === local.defaultInclude.trim() ? "" : include;
      await api.worktreeLocalSet(space.id, own, setup);
      requestWorktreeSetup(null);
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
        void save();
      }}
    >
      <div className="grid gap-1.5">
        <Label htmlFor="worktree-include">Local files to copy</Label>
        <Textarea
          id="worktree-include"
          value={include}
          onChange={(e) => setInclude(e.target.value)}
          disabled={!local}
          spellCheck={false}
          autoComplete="off"
          rows={8}
          className="font-mono text-[12px] md:text-[12px]"
        />
        <p className="text-[12px] text-muted-foreground">
          One pattern per line, gitignore syntax. Only files your repo ignores are copied, never tracked ones. Start a line with ! to leave files out, like !.claude/hooks/slow.sh. Lines starting with # are notes.
          {local?.hasFile && " This repo has a .worktreeinclude, so that file is used and this list is ignored."}
          {local && !local.hasFile && " A .worktreeinclude in the repo would take precedence over this list."}
        </p>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="worktree-setup">Setup command</Label>
        <Input
          id="worktree-setup"
          value={setup}
          onChange={(e) => setSetup(e.target.value)}
          disabled={!local}
          placeholder="pnpm install"
          spellCheck={false}
          autoComplete="off"
          className="font-mono text-[12px] md:text-[12px]"
        />
        <p className="text-[12px] text-muted-foreground">
          Any shell line, like ./scripts/setup-worktree.sh or make bootstrap && pnpm install. It runs in the new worktree's folder, in the first terminal opened there, and the agent starts once it succeeds.
        </p>
      </div>
      {error && (
        <p role="alert" className="max-h-32 overflow-y-auto rounded-lg bg-destructive/10 px-3 py-2 text-[12px] break-words whitespace-pre-wrap text-destructive">
          {error}
        </p>
      )}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={() => requestWorktreeSetup(null)}>
          Cancel
        </Button>
        <Button type="submit" disabled={busy || !local}>
          {busy && <Spinner />}
          Save
        </Button>
      </DialogFooter>
    </form>
  );
}

/** What a project's new worktrees get: its local files and its setup command. Opened by `requestWorktreeSetup`. */
export function WorktreeSetupDialog({ spaces }: { spaces: Space[] }) {
  const spaceId = useSyncExternalStore(worktreeSetupStore.subscribe, worktreeSetupStore.get);
  const space = spaces.find((s) => s.id === spaceId);
  return (
    <Dialog open={!!space} onOpenChange={(open) => !open && requestWorktreeSetup(null)}>
      <DialogContent>
        <div className="grid gap-1">
          <DialogTitle>Worktree setup</DialogTitle>
          <DialogDescription>What a new worktree of {space?.name ?? "the project"} gets before its agent starts.</DialogDescription>
        </div>
        {space && <Form key={space.id} space={space} />}
      </DialogContent>
    </Dialog>
  );
}
