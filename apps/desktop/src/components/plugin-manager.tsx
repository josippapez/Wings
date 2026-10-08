import { useEffect, useState } from "react";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { AnimatePresence, motion } from "motion/react";
import { CircleAlertIcon, CircleArrowUpIcon, EllipsisIcon, FileArchiveIcon, PuzzleIcon, TrashIcon } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { api, type PluginView } from "@/lib/api";
import { cn } from "@/lib/utils";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const uiNames: Record<string, string> = { badges: "Pane badges", diff: "Diff viewer" };

/** What a plugin adds, as short chips: UI parts, MCP tools for Claude, and whether it runs commands. */
function Adds({ plugin }: { plugin: PluginView }) {
  const { ui, mcpTools, panels, sidebars } = plugin.contributes;
  const chips = [
    ...ui.map((kind) => ({ key: kind, label: uiNames[kind] ?? kind, title: undefined as string | undefined })),
    ...sidebars.map((s) => ({ key: `sidebar:${s.id}`, label: `${s.title} sidebar`, title: undefined })),
    ...panels.map((p) => ({ key: `panel:${p.id}`, label: `${p.title} panel`, title: p.url })),
    ...(plugin.permissions.fetch.length ? [{ key: "fetch", label: "Uses the web", title: plugin.permissions.fetch.join(", ") }] : []),
    ...(mcpTools.length
      ? [{ key: "mcp", label: `${mcpTools.length} MCP ${mcpTools.length === 1 ? "tool" : "tools"}`, title: mcpTools.map((t) => t.name).join(", ") }]
      : []),
    ...(plugin.permissions.exec.length ? [{ key: "exec", label: "Runs commands", title: plugin.permissions.exec.join(", ") }] : []),
  ];
  if (chips.length === 0) return null;
  return (
    <ul className="mt-1.5 flex flex-wrap gap-1" aria-label="What it adds">
      {chips.map((c) => (
        <li key={c.key}>
          <Badge variant="secondary" title={c.title} className="h-auto px-2 py-px text-[11px] font-normal text-muted-foreground">
            {c.label}
          </Badge>
        </li>
      ))}
    </ul>
  );
}

function sourceLabel(p: PluginView) {
  if (p.dev) return "From this repo (development build)";
  if (p.source?.kind === "github") return `From GitHub, ${p.source.repo}`;
  return "From a file";
}

/** What turning a plugin on lets it do, in plain words, so you know what you're approving. */
function Permissions({ plugin }: { plugin: PluginView }) {
  const { exec, transcript, openUrl, fetch } = plugin.permissions;
  const { ui, mcpTools, panels, sidebars } = plugin.contributes;
  const groups = [
    { title: "Show these in Wings", items: [...ui.map((kind) => uiNames[kind] ?? kind), ...sidebars.map((s) => `${s.title} sidebar`)] },
    { title: "Call these web addresses, with tokens you give it", items: fetch },
    { title: "Open these sites in a panel from the title bar", items: panels.map((p) => new URL(p.url).host) },
    { title: "Offer Claude these tools over MCP", items: mcpTools.map((t) => t.name) },
    { title: "Run these commands as you, with your own sign-ins", items: exec },
    { title: "Read these parts of your Claude Code sessions", items: transcript },
    { title: "Open links that start with", items: openUrl },
  ].filter((g) => g.items.length > 0);
  if (groups.length === 0) return <p className="text-[13px] text-muted-foreground">It doesn't ask for any access beyond showing things in Wings.</p>;
  return (
    <div className="flex flex-col gap-3">
      {groups.map((g) => (
        <div key={g.title}>
          <p className="mb-1.5 text-[12px] font-medium text-muted-foreground">{g.title}</p>
          <ul className="flex flex-wrap gap-1.5">
            {g.items.map((item) => (
              <li key={item} className="rounded-md bg-white/[0.06] px-1.5 py-0.5 font-mono text-[12px]">
                {item}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

function Approve(props: { plugin: PluginView | null; busy: boolean; onCancel: () => void; onApprove: () => void }) {
  const p = props.plugin;
  return (
    <Dialog open={p !== null} onOpenChange={(open) => !open && props.onCancel()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Turn on {p?.name}?</DialogTitle>
          <DialogDescription>
            {p?.approved === false && p.enabled ? "This version asks for different access than you approved before." : "Here's what it will be able to do."}
          </DialogDescription>
        </DialogHeader>
        {p && <Permissions plugin={p} />}
        {p && p.contributes.mcpTools.length > 0 && (
          <p className="text-[12px] text-muted-foreground">Wings doesn't serve MCP tools to Claude yet. These start working once it does.</p>
        )}
        <p className="text-[12px] text-muted-foreground">Commands run as you, so only turn on plugins you trust.</p>
        <DialogFooter>
          <Button variant="ghost" onClick={props.onCancel}>
            Cancel
          </Button>
          <Button onClick={props.onApprove} disabled={props.busy}>
            {props.busy && <Spinner />}
            Turn on
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function PluginManager(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  plugins: PluginView[];
  /** A dropped `.wings-plugin` file waiting to be installed. */
  dropped: string | null;
  onDroppedHandled: () => void;
  /** New list from Rust; `restart` names a plugin that was just reinstalled. */
  onChanged: (plugins: PluginView[], restart?: string) => void;
}) {
  const [link, setLink] = useState("");
  const [installing, setInstalling] = useState<"link" | "file" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Set<string>>(() => new Set());
  const [rowError, setRowError] = useState<Record<string, string>>({});
  const [updates, setUpdates] = useState<Record<string, string>>({});
  const [approving, setApproving] = useState<PluginView | null>(null);

  const refresh = async (restart?: string) => props.onChanged(await api.pluginsList(), restart);

  async function work(id: string, task: () => Promise<unknown>) {
    setBusy((s) => new Set(s).add(id));
    setRowError(({ [id]: _old, ...rest }) => rest);
    try {
      await task();
    } catch (e) {
      setRowError((all) => ({ ...all, [id]: message(e) }));
    } finally {
      setBusy((s) => {
        const next = new Set(s);
        next.delete(id);
        return next;
      });
    }
  }

  async function install(kind: "link" | "file", run: () => Promise<PluginView>) {
    setInstalling(kind);
    setError(null);
    try {
      const plugin = await run();
      await refresh(plugin.id);
      if (kind === "link") setLink("");
      // A new plugin, or one whose update asks for more, waits for you to approve it.
      if (!plugin.approved || !plugin.enabled) setApproving(plugin);
    } catch (e) {
      setError(message(e));
    } finally {
      setInstalling(null);
    }
  }

  async function chooseFile() {
    const path = await openFile({ multiple: false, title: "Install a Wings plugin", filters: [{ name: "Wings plugin", extensions: ["wings-plugin"] }] });
    if (typeof path === "string") await install("file", () => api.pluginInstallFile(path));
  }

  const { dropped, onDroppedHandled } = props;
  useEffect(() => {
    if (!dropped) return;
    onDroppedHandled();
    void install("file", () => api.pluginInstallFile(dropped));
  }, [dropped]);

  // Look for newer releases of GitHub plugins each time the manager opens.
  const githubIds = props.plugins.filter((p) => p.source?.kind === "github").map((p) => p.id).join(",");
  useEffect(() => {
    if (!props.open || !githubIds) return;
    for (const id of githubIds.split(",")) {
      void api.pluginLatestVersion(id).then(
        (latest) => setUpdates((all) => ({ ...all, [id]: latest ?? "" })),
        () => {},
      );
    }
  }, [props.open, githubIds]);

  function toggle(plugin: PluginView, on: boolean) {
    if (on && !plugin.approved) return setApproving(plugin);
    void work(plugin.id, async () => {
      await api.pluginSetEnabled(plugin, on);
      await refresh();
    });
  }

  async function approve() {
    if (!approving) return;
    const shown = approving;
    await work(shown.id, async () => {
      await api.pluginSetEnabled(shown, true);
      await refresh(shown.id);
    });
    setApproving(null);
  }

  return (
    <>
      <Sheet open={props.open} onOpenChange={props.onOpenChange}>
        <SheetContent side="right" className="flex w-[440px] flex-col gap-0 p-0 sm:max-w-[440px]">
          <SheetHeader className="border-b border-hairline px-5 pt-5 pb-4">
            <SheetTitle>Plugins</SheetTitle>
            <SheetDescription>Plugins add features to Wings. Each one runs only after you approve what it can do.</SheetDescription>
          </SheetHeader>

          <div className="flex flex-col gap-2.5 border-b border-hairline px-5 py-4">
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (link.trim()) void install("link", () => api.pluginInstallGithub(link));
              }}
            >
              <Input
                value={link}
                onChange={(e) => setLink(e.target.value)}
                placeholder="github.com/owner/plugin"
                aria-label="GitHub repo link"
                spellCheck={false}
                className="h-8 text-[13px]"
              />
              <Button type="submit" size="sm" disabled={!link.trim() || installing !== null} className="h-8">
                {installing === "link" && <Spinner />}
                Install
              </Button>
            </form>
            <div className="flex items-center gap-2 text-[12px] text-muted-foreground">
              <Button variant="outline" size="sm" onClick={() => void chooseFile()} disabled={installing !== null} className="h-7">
                {installing === "file" ? <Spinner /> : <FileArchiveIcon aria-hidden />}
                Choose a file
              </Button>
              <span>or drop a .wings-plugin file on the window</span>
            </div>
            <AnimatePresence>
              {error && (
                <motion.p
                  role="alert"
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: "auto" }}
                  exit={{ opacity: 0, height: 0 }}
                  className="flex items-start gap-1.5 text-[12px] text-blocked"
                >
                  <CircleAlertIcon className="mt-px size-3.5 shrink-0" aria-hidden />
                  {error}
                </motion.p>
              )}
            </AnimatePresence>
          </div>

          <ScrollArea className="min-h-0 flex-1">
            <ul className="flex flex-col gap-1 p-3">
              {props.plugins.length === 0 && (
                <li>
                  <Empty>
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <PuzzleIcon />
                      </EmptyMedia>
                      <EmptyTitle>No plugins yet</EmptyTitle>
                      <EmptyDescription>Paste a GitHub link or drop a .wings-plugin file to add one.</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                </li>
              )}
              {props.plugins.map((p) => {
                const pending = busy.has(p.id);
                const latest = updates[p.id];
                const update = latest && latest !== p.version.replace(/^v/, "") ? latest : null;
                return (
                  <li key={p.id} className="rounded-xl px-3 py-3 transition-colors duration-150 hover:bg-hover">
                    <div className="flex items-start gap-3">
                      <span className={cn("mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg bg-white/[0.06]", p.enabled && p.approved && "bg-[#b392f0]/15 text-[#c9b2f7]")}>
                        <PuzzleIcon className="size-4" aria-hidden />
                      </span>
                      <div className="min-w-0 flex-1">
                        <p className="flex items-baseline gap-2">
                          <span className="truncate text-[13px] font-semibold">{p.name}</span>
                          <span className="shrink-0 text-[12px] text-muted-foreground tabular-nums">{p.version}</span>
                        </p>
                        {p.description && <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-muted-foreground">{p.description}</p>}
                        <Adds plugin={p} />
                        <p className="mt-1 text-[11px] text-muted-foreground">{sourceLabel(p)}</p>
                        {!p.approved && (
                          <Button variant="link" size="xs" onClick={() => setApproving(p)} className="mt-1 h-auto px-0 text-[12px] text-working">
                            Review access to turn it on
                          </Button>
                        )}
                        {update && (
                          <Button
                            variant="secondary"
                            size="sm"
                            className="mt-2 h-7"
                            disabled={pending}
                            onClick={() =>
                              void work(p.id, async () => {
                                const next = await api.pluginUpdate(p.id);
                                await refresh(p.id);
                                if (!next.approved) setApproving(next);
                              })
                            }
                          >
                            <CircleArrowUpIcon aria-hidden />
                            Update to {update}
                          </Button>
                        )}
                        {rowError[p.id] && (
                          <p role="alert" className="mt-1.5 text-[12px] text-blocked">
                            {rowError[p.id]}
                          </p>
                        )}
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        {pending && <Spinner className="size-3.5 text-muted-foreground" aria-label="Working" />}
                        <Switch
                          checked={p.enabled && p.approved}
                          onCheckedChange={(on) => toggle(p, on)}
                          disabled={pending}
                          aria-label={`${p.name} ${p.enabled && p.approved ? "on" : "off"}`}
                        />
                        {!p.dev && (
                          <DropdownMenu>
                            <DropdownMenuTrigger
                              render={<Button variant="ghost" size="icon-sm" aria-label={`More for ${p.name}`} className="text-muted-foreground" />}
                            >
                              <EllipsisIcon aria-hidden />
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              {p.source?.kind === "github" && (
                                <>
                                  <DropdownMenuItem
                                    onClick={() =>
                                      void work(p.id, async () => {
                                        const next = await api.pluginUpdate(p.id);
                                        await refresh(p.id);
                                        if (!next.approved) setApproving(next);
                                      })
                                    }
                                  >
                                    Reinstall from GitHub
                                  </DropdownMenuItem>
                                  <DropdownMenuSeparator />
                                </>
                              )}
                              <DropdownMenuItem
                                variant="destructive"
                                onClick={() =>
                                  void work(p.id, async () => {
                                    await api.pluginRemove(p.id);
                                    await refresh();
                                  })
                                }
                              >
                                <TrashIcon aria-hidden />
                                Remove
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        )}
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          </ScrollArea>
        </SheetContent>
      </Sheet>
      <Approve plugin={approving} busy={approving ? busy.has(approving.id) : false} onCancel={() => setApproving(null)} onApprove={() => void approve()} />
    </>
  );
}
