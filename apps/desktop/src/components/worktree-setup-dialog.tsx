import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
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

  // What the form was last filled with, so a change from outside only replaces fields nobody has edited.
  const loaded = useRef({ include: "", setup: "" });
  const current = useRef({ include, setup });
  current.current = { include, setup };

  const load = useCallback(
    (isGone: () => boolean) =>
      api
        .worktreeLocal(space.id)
        .then((got) => {
          if (isGone()) return;
          const next = { include: got.include ?? got.defaultInclude, setup: got.setup ?? "" };
          setLocal(got);
          if (current.current.include === loaded.current.include) setInclude(next.include);
          if (current.current.setup === loaded.current.setup) setSetup(next.setup);
          loaded.current = next;
        })
        .catch((e) => !isGone() && setError(String(e))),
    [space.id],
  );

  useEffect(() => {
    let gone = false;
    void load(() => gone);
    const off = api.onWorktreeSetupChanged(() => void load(() => gone));
    return () => {
      gone = true;
      void off.then((unlisten) => unlisten());
    };
  }, [load]);

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
      <Field className="gap-1.5">
        <FieldLabel className="leading-none" htmlFor="worktree-include">Local files to copy</FieldLabel>
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
        <FieldDescription className="text-[12px]">
          One pattern per line, gitignore syntax. Only files your repo ignores are copied, never tracked ones. Start a line with ! to leave files out, like !.claude/hooks/slow.sh. Lines starting with # are notes.
          {local?.hasFile && " This repo has a .worktreeinclude, so that file is used and this list is ignored."}
          {local && !local.hasFile && " A .worktreeinclude in the repo would take precedence over this list."}
        </FieldDescription>
      </Field>
      <Field className="gap-1.5">
        <FieldLabel className="leading-none" htmlFor="worktree-setup">Setup command</FieldLabel>
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
        <FieldDescription className="text-[12px]">
          Any shell line, like ./scripts/setup-worktree.sh or make bootstrap && pnpm install. It runs in the new worktree's folder, in the first terminal opened there, and the agent starts once it succeeds.
        </FieldDescription>
      </Field>
      {error && (
        <Alert variant="destructive" className="max-h-32 overflow-y-auto border-transparent bg-destructive/10 px-3 text-[12px]">
          <AlertDescription className="text-[12px] text-wrap break-words md:text-wrap whitespace-pre-wrap">{error}</AlertDescription>
        </Alert>
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
