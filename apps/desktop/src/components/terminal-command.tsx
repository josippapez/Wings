import { useEffect, useState } from "react";
import { CircleCheckIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { api, type CliStatus } from "@/lib/api";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const pathLine = 'export PATH="$HOME/.local/bin:$PATH"';

/** What to do once the command is added, which depends on whether its folder is on PATH. */
function Added({ status }: { status: CliStatus }) {
  if (status.onPath) {
    return (
      <p className="text-[13px] text-muted-foreground">
        Added. Open a new terminal and try <code className="font-mono text-foreground">wings plugin list</code>.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2 text-[13px] text-muted-foreground">
      <p>Added to ~/.local/bin, but that folder isn't on your PATH yet. Add this line to your shell profile, like ~/.zshrc:</p>
      <code className="rounded-md bg-white/[0.06] px-2 py-1 font-mono text-[12px] text-foreground select-all">{pathLine}</code>
    </div>
  );
}

/** Asks once, on first start, whether to add the `wings` command. */
export function TerminalCommandPrompt() {
  const [status, setStatus] = useState<CliStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState<CliStatus | null>(null);

  useEffect(() => {
    void api.cliStatus().then((s) => {
      setStatus(s);
      setOpen(s.available && !s.installed && !s.asked);
    }, () => {});
  }, []);

  async function add() {
    setAdding(true);
    setError(null);
    try {
      setAdded(await api.cliInstall());
    } catch (e) {
      setError(message(e));
    } finally {
      setAdding(false);
    }
  }

  function close() {
    setOpen(false);
    if (!added) void api.cliDismiss().catch(() => {});
  }

  if (!status) return null;
  return (
    <Dialog open={open} onOpenChange={(next) => !next && close()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Use Wings from the terminal?</DialogTitle>
          <DialogDescription>
            This adds a <code className="font-mono">wings</code> command, so you, your scripts and Claude can manage plugins, like{" "}
            <code className="font-mono">wings plugin install github.com/owner/name</code>.
          </DialogDescription>
        </DialogHeader>
        {added && <Added status={added} />}
        {error && (
          <p role="alert" className="text-[12px] text-blocked">
            {error}
          </p>
        )}
        <DialogFooter>
          {added ? (
            <Button onClick={close}>Done</Button>
          ) : (
            <>
              <Button variant="ghost" onClick={close}>
                Not now
              </Button>
              <Button onClick={() => void add()} disabled={adding}>
                {adding && <Spinner />}
                Add command
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The same choice in the Plugins sheet, for later or after Wings moved. */
export function TerminalCommandRow({ open }: { open: boolean }) {
  const [status, setStatus] = useState<CliStatus | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) void api.cliStatus().then(setStatus, () => {});
  }, [open]);

  async function add() {
    setAdding(true);
    setError(null);
    try {
      setStatus(await api.cliInstall());
    } catch (e) {
      setError(message(e));
    } finally {
      setAdding(false);
    }
  }

  if (!status?.available) return null;
  return (
    <div className="flex flex-col gap-1.5 border-b border-hairline px-5 py-3">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-1.5 text-[13px] font-medium">
            Terminal command
            {status.installed && <CircleCheckIcon className="size-3.5 text-done" aria-label="Added" />}
          </p>
          <p className="text-[12px] leading-snug text-muted-foreground">
            {!status.installed ? (
              "Add a wings command, so scripts and Claude can install plugins."
            ) : status.onPath ? (
              <>
                Run <code className="font-mono">wings plugin</code> in any terminal.
              </>
            ) : (
              <>
                It's in ~/.local/bin, which isn't on your PATH. Add <code className="font-mono select-all">{pathLine}</code> to your shell profile.
              </>
            )}
          </p>
        </div>
        {!status.installed && (
          <Button variant="secondary" size="sm" className="h-7 shrink-0" disabled={adding} onClick={() => void add()}>
            {adding && <Spinner />}
            Add
          </Button>
        )}
      </div>
      {error && (
        <p role="alert" className="text-[12px] text-blocked">
          {error}
        </p>
      )}
    </div>
  );
}
