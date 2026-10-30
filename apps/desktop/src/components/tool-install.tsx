import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleCheckIcon, ExternalLinkIcon } from "lucide-react";
import { toast } from "sonner";

import { CodeSnippet } from "@/components/code-snippet";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { api, type PaneInfo, type PluginView, type ToolStatus } from "@/lib/api";
import { onToolInstall, requestToolInstall, settleInstalls, type InstallWatch, type ToolRequest } from "@/lib/tool-install";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const HOMEBREW = "https://brew.sh";

/**
 * Asks before installing a tool a plugin needs, then types Rust's install command for it into a new tab in the
 * project on screen, so you see Homebrew's output and can answer it. Once the command is done (the tab is back at
 * its prompt, or closed) it checks again and says so.
 */
export function ToolInstallDialog(props: {
  project: { id: string; name: string } | null;
  paneInfo: Record<string, PaneInfo>;
  /** Opens a tab in the project with `input` typed into its shell; resolves its pane id. */
  openTab: (spaceId: string, input: string) => Promise<string | null>;
  /** The install is running in its tab, so the tab should be on screen. */
  onStarted: () => void;
  /** The tool is there now: the plugins that run it start again. */
  onInstalled: (program: string) => void;
}) {
  const [asking, setAsking] = useState<(ToolRequest & { answer: (started: boolean) => void }) | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const watches = useRef<InstallWatch[]>([]);
  const latest = useRef(props);
  latest.current = props;

  useEffect(
    () =>
      onToolInstall(async (request) => {
        if (request.tool.installed) {
          latest.current.onInstalled(request.tool.program);
          return true;
        }
        // Its install is already running in a tab.
        if (watches.current.some((w) => w.tool.program === request.tool.program)) return true;
        return new Promise<boolean>((answer) =>
          setAsking((open) => {
            open?.answer(false);
            return { ...request, answer };
          }),
        );
      }),
    [],
  );

  // Pane info only changes when a pane's program or folder does, so this runs on those changes, not on a timer.
  const { paneInfo } = props;
  useEffect(() => {
    if (watches.current.length === 0) return;
    const { running, done } = settleInstalls(watches.current, paneInfo);
    watches.current = running;
    for (const watch of done) void check(watch);
  }, [paneInfo]);

  async function check({ pluginId, tool }: InstallWatch) {
    const now = await api.pluginTools(pluginId).catch(() => []);
    if (now.find((t) => t.program === tool.program)?.installed) {
      toast.success(`${tool.name} is installed`);
      latest.current.onInstalled(tool.program);
    } else {
      toast.error(`${tool.name} isn't installed`, { description: "Its tab shows what Homebrew said." });
    }
  }

  function close(started: boolean) {
    asking?.answer(started);
    setAsking(null);
    setError(null);
  }

  async function install() {
    if (!asking?.tool.command || !props.project) return;
    setStarting(true);
    setError(null);
    try {
      // The command comes from Rust's own table, never from the plugin.
      const paneId = await props.openTab(props.project.id, `${asking.tool.command}\r`);
      if (!paneId) throw new Error("The tab didn't start");
      watches.current = [...watches.current, { ...asking, paneId, seen: false, ran: false }];
      close(true);
      props.onStarted();
    } catch (e) {
      setError(message(e));
    } finally {
      setStarting(false);
    }
  }

  const tool = asking?.tool;
  return (
    <Dialog open={asking !== null} onOpenChange={(open) => !open && close(false)}>
      <DialogContent className="sm:max-w-md" data-testid="tool-install-dialog">
        <DialogHeader>
          <DialogTitle>Install {tool?.name}?</DialogTitle>
          <DialogDescription>
            {asking?.pluginName} runs {tool?.name} (<code className="font-mono">{tool?.program}</code>), which isn't installed.
          </DialogDescription>
        </DialogHeader>
        {tool?.command ? (
          <div className="flex flex-col gap-2 text-[13px] text-muted-foreground">
            <p>
              {props.project
                ? `Wings opens a new tab in ${props.project.name} and runs this there, so you can follow it and answer any question it asks:`
                : "Add a project to Wings first. The install runs in a new tab there:"}
            </p>
            <CodeSnippet code={tool.command} language="bash" wrap className="text-foreground" />
          </div>
        ) : tool?.needsHomebrew ? (
          <p className="text-[13px] text-muted-foreground">Wings installs it with Homebrew, which isn't on this Mac. Install Homebrew first, or follow {tool.name}'s own guide.</p>
        ) : (
          <p className="text-[13px] text-muted-foreground">Follow {tool?.name}'s install guide, then come back.</p>
        )}
        {error && (
          <p role="alert" className="text-[12px] text-blocked">
            {error}
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => close(false)}>
            Cancel
          </Button>
          {tool?.needsHomebrew && (
            <Button variant="outline" onClick={() => void openUrl(HOMEBREW)}>
              <ExternalLinkIcon aria-hidden />
              Get Homebrew
            </Button>
          )}
          <Button variant={tool?.command ? "outline" : "default"} onClick={() => tool && void openUrl(tool.docs)}>
            <ExternalLinkIcon aria-hidden />
            How to install
          </Button>
          {tool?.command && (
            <Button onClick={() => void install()} disabled={starting || !props.project}>
              {starting && <Spinner />}
              Install with Homebrew
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The plugin's page in Settings: each tool it runs, and whether it's installed, with a way to install it. */
export function PluginNeeds({ plugin }: { plugin: PluginView }) {
  const [tools, setTools] = useState<ToolStatus[]>([]);

  // Again whenever the plugin list refreshes, which it does after an install.
  useEffect(() => {
    let stale = false;
    void api.pluginTools(plugin.id).then(
      (list) => !stale && setTools(list),
      () => !stale && setTools([]),
    );
    return () => {
      stale = true;
    };
  }, [plugin]);

  if (tools.length === 0) return null;
  return (
    <section className="flex flex-col gap-2" data-testid="plugin-needs">
      <h5 className="text-[13px] font-semibold">Needs</h5>
      <ul className="flex flex-col gap-1.5">
        {tools.map((tool) => (
          <li key={tool.program} className="flex min-h-7 flex-wrap items-center gap-x-2 gap-y-1 text-[13px]">
            <span>{tool.name}</span>
            <code className="rounded-md bg-white/[0.06] px-1.5 py-0.5 font-mono text-[12px]">{tool.program}</code>
            {tool.installed ? (
              <span className="flex items-center gap-1 text-[12px] text-muted-foreground">
                <CircleCheckIcon className="size-3.5 text-done" aria-hidden />
                Installed
              </span>
            ) : (
              <>
                <span className="text-[12px] text-working">Not installed</span>
                <Button
                  variant="secondary"
                  size="sm"
                  className="ml-auto h-7"
                  onClick={() => void requestToolInstall({ pluginId: plugin.id, pluginName: plugin.name, tool }).catch((e) => toast.error(message(e)))}
                >
                  {tool.command ? "Install with Homebrew" : "How to install"}
                </Button>
              </>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
