import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLinkIcon, RefreshCwIcon } from "lucide-react";

import { ClaudeIcon } from "@/components/claude-icon";
import { AgentConnectionChip } from "@/components/connections";
import { OpencodeIcon } from "@/components/opencode-icon";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { api, type HarnessStatus } from "@/lib/api";
import { cn } from "@/lib/utils";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

function HarnessIcon({ id, name }: { id: string; name: string }) {
  if (id === "claude") return <ClaudeIcon className="size-4 text-[#d97757]" aria-hidden />;
  if (id === "opencode") return <OpencodeIcon className="size-4 text-foreground" aria-hidden />;
  return (
    <span className="flex size-4 items-center justify-center rounded-[4px] bg-white/[0.1] text-[10px] font-semibold text-foreground" aria-hidden>
      {name[0]}
    </span>
  );
}

function state(h: HarnessStatus): { text: string; tone: string } {
  if (!h.installed) return { text: "Not installed", tone: "bg-muted-foreground/50" };
  if (h.auth === "signedIn") return { text: h.account ? `Signed in · ${h.account}` : "Signed in", tone: "bg-done" };
  if (h.auth === "signedOut") return { text: "Not signed in", tone: "bg-working" };
  return { text: h.note ?? "Installed", tone: "bg-muted-foreground/50" };
}

/**
 * The coding agents Wings knows, whether each is installed and signed in, with a way to install it or sign in.
 * Each check runs the agent's own CLI, so they run when this page opens and on Refresh, never on a timer.
 */
export function Harnesses(props: {
  open: boolean;
  /** Opens a tab in the project on screen with `input` typed into it; null with no project open. */
  onRunInTab: ((input: string) => void) | null;
}) {
  const [list, setList] = useState<HarnessStatus[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const check = () => {
    setChecking(true);
    setError(null);
    void api
      .harnesses()
      .then(setList, (e) => setError(message(e)))
      .finally(() => setChecking(false));
  };
  useEffect(() => {
    if (props.open) check();
  }, [props.open]);

  const noProject = props.onRunInTab ? undefined : "Open a project first. It runs in a new tab there.";
  return (
    <section className="flex flex-col gap-2" aria-labelledby="harnesses-title">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h4 id="harnesses-title" className="text-[13px] font-medium">
            Coding agents
          </h4>
          <p className="text-[12px] text-muted-foreground">The agent CLIs on this Mac, and whether each is signed in.</p>
        </div>
        <Button variant="ghost" size="xs" className="h-6 text-[12px]" disabled={checking} onClick={check}>
          {checking ? <Spinner /> : <RefreshCwIcon aria-hidden />}
          Refresh
        </Button>
      </div>
      <div className="divide-y divide-hairline rounded-xl border border-hairline bg-white/[0.03]" aria-busy={checking}>
        {list === null
          ? Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="flex items-center gap-3 px-4 py-3" aria-hidden>
                <Skeleton className="size-4 bg-white/[0.06]" />
                <div className="flex flex-1 flex-col gap-1.5">
                  <Skeleton className="h-3.5 w-28 bg-white/[0.06]" />
                  <Skeleton className="h-3 w-40 bg-white/[0.04]" />
                </div>
              </div>
            ))
          : list.map((h) => {
              const { text, tone } = state(h);
              return (
                <div key={h.id} className="flex items-center gap-3 px-4 py-3" data-testid={`harness-${h.id}`}>
                  <HarnessIcon id={h.id} name={h.name} />
                  <div className="min-w-0 flex-1">
                    <p className="flex items-baseline gap-2 text-[13px]">
                      {h.name}
                      {h.version && <code className="truncate font-mono text-[11px] text-muted-foreground">{h.version}</code>}
                    </p>
                    <p className="mt-0.5 flex items-center gap-1.5 text-[12px] text-muted-foreground">
                      <span className={cn("size-1.5 shrink-0 rounded-full", tone)} aria-hidden />
                      <span className="truncate" title={text}>
                        {text}
                      </span>
                    </p>
                  </div>
                  {h.installed && (h.id === "claude" || h.id === "opencode") && <AgentConnectionChip id={h.id} label="Wings tools" open={props.open} />}
                  {!h.installed && (
                    <>
                      <Button variant="ghost" size="icon-sm" aria-label={`How to install ${h.name}`} title={`How to install ${h.name}`} onClick={() => void openUrl(h.docs)}>
                        <ExternalLinkIcon aria-hidden />
                      </Button>
                      <Button
                        variant="secondary"
                        size="sm"
                        className="h-7"
                        disabled={!props.onRunInTab}
                        // Typed but not run: you read the command and press Enter.
                        title={noProject ?? `Types ${h.install} into a new tab, for you to run`}
                        onClick={() => props.onRunInTab?.(h.install)}
                      >
                        Install
                      </Button>
                    </>
                  )}
                  {h.installed && h.auth === "signedOut" && (
                    <Button variant="secondary" size="sm" className="h-7" disabled={!props.onRunInTab} title={noProject ?? `Runs ${h.login} in a new tab`} onClick={() => props.onRunInTab?.(`${h.login}\r`)}>
                      Sign in
                    </Button>
                  )}
                </div>
              );
            })}
      </div>
      {error && (
        <p role="alert" className="text-[12px] text-blocked">
          Couldn't check the agents: {error}
        </p>
      )}
    </section>
  );
}
