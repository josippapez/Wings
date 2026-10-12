import { useEffect, useState } from "react";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AnimatePresence, motion } from "motion/react";
import { CircleAlertIcon, CircleArrowUpIcon, CircleCheckIcon, FileArchiveIcon, PuzzleIcon, RefreshCwIcon, TrashIcon } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Markdown } from "@/components/markdown";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { TerminalCommandRow } from "@/components/terminal-command";
import { PluginNeeds } from "@/components/tool-install";
import { api, type McpStatus, type PluginPage, type PluginSidebar, type PluginView } from "@/lib/api";
import { newerVersion } from "@/lib/plugin-updates";
import { cn } from "@/lib/utils";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const uiNames: Record<string, string> = { badges: "Pane badges", diff: "Diff viewer" };
/** Like "Board modal or window". */
const viewName = (v: PluginPage) => `${v.title} ${v.opens.join(" or ")}`;
/** Like "Timer sidebar and popover". */
const sidebarName = (s: PluginSidebar) => `${s.title} sidebar${s.popover ? " and popover" : ""}`;

/** What a plugin adds, as short chips: UI parts, MCP tools for Claude, and whether it runs commands. */
function Adds({ plugin }: { plugin: PluginView }) {
  const { ui, mcpTools, panels, sidebars, views } = plugin.contributes;
  const chips = [
    ...ui.map((kind) => ({ key: kind, label: uiNames[kind] ?? kind, title: undefined as string | undefined })),
    ...sidebars.map((s) => ({ key: `sidebar:${s.id}`, label: sidebarName(s), title: undefined })),
    ...views.map((v) => ({ key: `view:${v.id}`, label: viewName(v), title: undefined })),
    ...panels.map((p) => ({ key: `panel:${p.id}`, label: `${p.title} panel`, title: p.url })),
    ...(plugin.permissions.fetch.length ? [{ key: "fetch", label: "Uses the web", title: plugin.permissions.fetch.join(", ") }] : []),
    ...(mcpTools.length
      ? [{ key: "mcp", label: `${mcpTools.length} MCP ${mcpTools.length === 1 ? "tool" : "tools"}`, title: mcpTools.map((t) => t.name).join(", ") }]
      : []),
    ...(plugin.permissions.exec.length ? [{ key: "exec", label: "Runs commands", title: plugin.permissions.exec.join(", ") }] : []),
    ...(plugin.permissions.panes ? [{ key: "panes", label: "Opens terminals", title: plugin.permissions.panes.join(", ") || undefined }] : []),
    ...(plugin.permissions.notify ? [{ key: "notify", label: "Notifications", title: undefined }] : []),
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

function PluginIcon({ plugin, className }: { plugin: PluginView; className: string }) {
  return (
    <span className={cn("flex shrink-0 items-center justify-center rounded-lg bg-white/[0.06]", plugin.enabled && plugin.approved && "bg-[#b392f0]/15 text-[#c9b2f7]", className)}>
      <PuzzleIcon className="size-1/2" aria-hidden />
    </span>
  );
}

/** Where the plugin came from; a GitHub repo is a link. */
function Source({ plugin: p }: { plugin: PluginView }) {
  if (p.dev) return "From this repo (development build)";
  if (p.source?.kind !== "github") return "From a file";
  const folder = p.source.folder;
  const url = folder ? `https://github.com/${p.source.repo}/tree/HEAD/${folder}` : `https://github.com/${p.source.repo}`;
  return (
    <>
      From GitHub,{" "}
      <a
        href={url}
        onClick={(e) => {
          e.preventDefault();
          void openUrl(url);
        }}
        className="text-working underline-offset-2 hover:underline"
      >
        {folder ? `${p.source.repo}/${folder}` : p.source.repo}
      </a>
    </>
  );
}

/** What turning a plugin on lets it do, in plain words, so you know what you're approving. */
function Permissions({ plugin }: { plugin: PluginView }) {
  const { exec, post, transcript, openUrl, fetch, panes, notify, statusline, ports } = plugin.permissions;
  const { ui, mcpTools, panels, sidebars, views } = plugin.contributes;
  // `on` is for a yes/no permission, which has no list to show.
  const groups: { title: string; items: string[]; on?: boolean }[] = [
    { title: "Show these in Wings", items: [...ui.map((kind) => uiNames[kind] ?? kind), ...sidebars.map(sidebarName), ...views.map(viewName)] },
    { title: "Call these web addresses, with tokens you give it", items: fetch },
    { title: "Open these sites in a panel from the title bar", items: panels.map((p) => new URL(p.url).host) },
    { title: "Offer Claude these tools over MCP", items: mcpTools.map((t) => t.name) },
    { title: "Run these commands as you, with your own sign-ins", items: exec },
    { title: "Post to these addresses as you, like replying to a review comment", items: post },
    { title: "Open terminals in your projects and move your typing to any pane", items: [], on: panes !== null },
    { title: "Start these commands in a new terminal, as you", items: panes ?? [] },
    { title: "Show desktop notifications, up to 3 a minute", items: [], on: notify },
    { title: "Read these parts of your Claude Code sessions", items: transcript },
    { title: "Read your Claude usage limits and each session's context and cache, from Claude Code's status line", items: [], on: statusline },
    { title: "See which local ports the programs in your terminals listen on", items: [], on: ports },
    { title: "Open links that start with", items: openUrl },
  ].filter((g) => g.items.length > 0 || g.on);
  if (groups.length === 0) return <p className="text-[13px] text-muted-foreground">It doesn't ask for any access beyond showing things in Wings.</p>;
  return (
    <div className="flex flex-col gap-3">
      {groups.map((g) => (
        <div key={g.title}>
          <p className="mb-1.5 text-[12px] font-medium text-muted-foreground">{g.title}</p>
          {g.items.length > 0 && (
            <ul className="flex flex-wrap gap-1.5">
              {g.items.map((item) => (
                <li key={item} className="max-w-full rounded-md bg-white/[0.06] px-1.5 py-0.5 font-mono text-[12px] break-all">
                  {item}
                </li>
              ))}
            </ul>
          )}
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

/** Wings adds itself to Claude Code when it starts: plugin tools for its sessions, and its status line for plugins. */
function ClaudeConnect({ open }: { open: boolean }) {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [connecting, setConnecting] = useState<"connect" | "disconnect" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) void api.mcpStatus().then(setStatus, () => {});
  }, [open]);

  async function run(action: "connect" | "disconnect") {
    setConnecting(action);
    setError(null);
    try {
      await (action === "connect" ? api.mcpConnect() : api.mcpDisconnect());
      setStatus(await api.mcpStatus());
    } catch (e) {
      setError(message(e));
    } finally {
      setConnecting(null);
    }
  }
  const connected = status?.connected && status.statusline;

  if (!status) return null;
  return (
    <div className="flex flex-col gap-1.5 border-b border-hairline px-5 py-3">
      <div className="flex flex-wrap items-center gap-x-1 gap-y-2">
        <div className="min-w-0 basis-full">
          <p className="flex items-center gap-1.5 text-[13px] font-medium">
            Claude Code
            {connected && <CircleCheckIcon className="size-3.5 text-done" aria-label="Connected" />}
          </p>
          <p className="text-[12px] leading-snug text-muted-foreground">
            {!status.claude
              ? "Install Claude Code to give it plugin tools."
              : connected
                ? "Its sessions get plugin tools, and plugins get its usage numbers. Your own status line still shows."
                : "Connect it so its sessions get plugin tools and plugins get its usage numbers. Your own status line keeps showing."}
          </p>
        </div>
        {status.claude && (
          <Button variant={connected ? "outline" : "secondary"} size="sm" className="h-7 shrink-0" disabled={connecting !== null} onClick={() => void run("connect")}>
            {connecting === "connect" && <Spinner />}
            {connected ? "Reconnect" : "Connect"}
          </Button>
        )}
        {(status.connected || status.statusline) && (
          <Button variant="ghost" size="sm" className="h-7 shrink-0" disabled={connecting !== null} onClick={() => void run("disconnect")}>
            {connecting === "disconnect" && <Spinner />}
            Disconnect
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

const statusOf = (p: PluginView) => (!p.approved ? "Waiting for approval" : p.enabled ? "On" : "Off");

/** The Plugins section of Settings: install at the top, installed plugins on the left, the selected one's page on the right. */
export function PluginsSection(props: {
  plugins: PluginView[];
  /** The plugin shown on the right; the first one when null or gone. */
  selected: string | null;
  onSelect: (id: string) => void;
  /** A dropped `.wings-plugin` file waiting to be installed. */
  dropped: string | null;
  onDroppedHandled: () => void;
  /** A plugin id to open the approval for, from the CLI. */
  review: string | null;
  onReviewHandled: () => void;
  /** New list from Rust; `restart` names a plugin that was just reinstalled. */
  onChanged: (plugins: PluginView[], restart?: string) => void;
  /** Newer versions found by the update check, by plugin id (empty string: none). */
  updates: Record<string, string>;
  checking: boolean;
  onCheck: () => void;
}) {
  const [link, setLink] = useState("");
  const [installing, setInstalling] = useState<"link" | "file" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Set<string>>(() => new Set());
  const [rowError, setRowError] = useState<Record<string, string>>({});
  const [approving, setApproving] = useState<PluginView | null>(null);
  const [readme, setReadme] = useState<{ id: string; version: string; text: string | null } | null>(null);

  const current = props.plugins.find((p) => p.id === props.selected) ?? props.plugins[0] ?? null;
  const updateOf = (p: PluginView) => newerVersion(p, props.updates[p.id]);

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
      props.onSelect(plugin.id);
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

  const { review, onReviewHandled } = props;
  useEffect(() => {
    const plugin = review && props.plugins.find((p) => p.id === review);
    if (!plugin) return;
    onReviewHandled();
    props.onSelect(plugin.id);
    setApproving(plugin);
  }, [review, props.plugins]);

  // The README comes from the plugin's folder, so it's read again when the version changes.
  const shownId = current?.id;
  const shownVersion = current?.version;
  useEffect(() => {
    if (!shownId || !shownVersion) return;
    let stale = false;
    void api.pluginReadme(shownId).then(
      (text) => !stale && setReadme({ id: shownId, version: shownVersion, text }),
      () => !stale && setReadme({ id: shownId, version: shownVersion, text: null }),
    );
    return () => {
      stale = true;
    };
  }, [shownId, shownVersion]);

  function toggle(plugin: PluginView, on: boolean) {
    if (on && !plugin.approved) return setApproving(plugin);
    void work(plugin.id, async () => {
      await api.pluginSetEnabled(plugin, on);
      await refresh();
    });
  }

  const reinstall = (p: PluginView) =>
    work(p.id, async () => {
      const next = await api.pluginUpdate(p.id);
      await refresh(p.id);
      if (!next.approved) setApproving(next);
    });

  async function approve() {
    if (!approving) return;
    const shown = approving;
    await work(shown.id, async () => {
      await api.pluginSetEnabled(shown, true);
      await refresh(shown.id);
    });
    setApproving(null);
  }

  const hasGithub = props.plugins.some((p) => p.source?.kind === "github");
  const pending = current ? busy.has(current.id) : false;
  const currentUpdate = current && updateOf(current);
  const currentReadme = current && readme?.id === current.id && readme.version === current.version ? readme.text : null;

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="plugins-section">
      <div className="flex flex-col gap-2.5 border-b border-hairline px-6 pt-5 pb-4">
        <div>
          <h3 className="text-[15px] font-semibold">Plugins</h3>
          <p className="mt-0.5 text-[12px] text-muted-foreground">Plugins add features to Wings. Each one runs only after you approve what it can do.</p>
        </div>
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
          <Button type="button" variant="outline" size="sm" onClick={() => void chooseFile()} disabled={installing !== null} className="h-8">
            {installing === "file" ? <Spinner /> : <FileArchiveIcon aria-hidden />}
            Choose a file
          </Button>
        </form>
        <p className="text-[12px] text-muted-foreground">You can also drop a .wings-plugin file on the window.</p>
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

      <div className="flex min-h-0 flex-1">
        <div className="flex w-80 shrink-0 flex-col border-r border-hairline">
          <div className="flex items-center justify-between gap-2 px-4 pt-3 pb-1">
            <p className="text-[12px] font-medium text-muted-foreground">Installed</p>
            <Button variant="ghost" size="xs" className="h-6 text-[12px]" disabled={props.checking || !hasGithub} onClick={props.onCheck}>
              {props.checking ? <Spinner /> : <RefreshCwIcon aria-hidden />}
              Check for updates
            </Button>
          </div>
          <ScrollArea className="min-h-0 flex-1">
            <ul className="flex flex-col gap-0.5 px-2 pb-2" aria-label="Installed plugins">
              {props.plugins.length === 0 && (
                <li className="px-2 py-4 text-[12px] text-muted-foreground">No plugins yet. Paste a GitHub link or drop a .wings-plugin file to add one.</li>
              )}
              {props.plugins.map((p) => {
                const selected = p.id === current?.id;
                const update = updateOf(p);
                return (
                  <li key={p.id} data-testid={`plugin-row-${p.id}`}>
                    <div className={cn("rounded-lg transition-colors duration-150", selected ? "bg-white/[0.07]" : "hover:bg-hover")}>
                      <button
                        type="button"
                        aria-current={selected || undefined}
                        onClick={() => props.onSelect(p.id)}
                        className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                      >
                        <PluginIcon plugin={p} className="size-8" />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-baseline gap-2">
                            <span className="truncate text-[13px] font-medium">{p.name}</span>
                            <span className="shrink-0 text-[11px] text-muted-foreground tabular-nums">{p.version}</span>
                          </span>
                          <span className={cn("block truncate text-[12px] text-muted-foreground", !p.approved && "text-working")}>
                            {statusOf(p)}
                            {update && <span className="text-working"> · Update available</span>}
                          </span>
                        </span>
                        {busy.has(p.id) && <Spinner className="size-3.5 text-muted-foreground" aria-label="Working" />}
                      </button>
                      <AnimatePresence initial={false}>
                        {update && (
                          <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto" }} exit={{ opacity: 0, height: 0 }} className="overflow-hidden">
                            <div className="px-2.5 pb-2">
                              <Button variant="secondary" size="sm" className="h-7 w-full" disabled={busy.has(p.id)} onClick={() => void reinstall(p)}>
                                {busy.has(p.id) ? <Spinner /> : <CircleArrowUpIcon aria-hidden />}
                                Update to {update}
                              </Button>
                            </div>
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  </li>
                );
              })}
            </ul>
          </ScrollArea>
          <div className="shrink-0 border-t border-hairline [&>div:last-child]:border-b-0 [&>div]:px-4">
            <ClaudeConnect open />
            <TerminalCommandRow open />
          </div>
        </div>

        <ScrollArea className="min-h-0 min-w-0 flex-1">
          {!current ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <PuzzleIcon />
                </EmptyMedia>
                <EmptyTitle>No plugins yet</EmptyTitle>
                <EmptyDescription>Paste a GitHub link or drop a .wings-plugin file to add one.</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <motion.div key={current.id} initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.15 }} className="flex flex-col gap-5 p-6" data-testid="plugin-details">
              <div className="flex items-start gap-3">
                <PluginIcon plugin={current} className="size-10" />
                <div className="min-w-0 flex-1">
                  <h4 className="flex items-baseline gap-2">
                    <span className="truncate text-[15px] font-semibold">{current.name}</span>
                    <span className="shrink-0 text-[12px] text-muted-foreground tabular-nums">{current.version}</span>
                  </h4>
                  <p className="mt-0.5 text-[12px] text-muted-foreground">
                    <Source plugin={current} />
                  </p>
                </div>
                {pending && <Spinner className="mt-1 size-3.5 text-muted-foreground" aria-label="Working" />}
                <Switch
                  checked={current.enabled && current.approved}
                  onCheckedChange={(on) => toggle(current, on)}
                  disabled={pending}
                  aria-label={`${current.name} ${current.enabled && current.approved ? "on" : "off"}`}
                  className="mt-1"
                />
              </div>

              {current.description && <p className="text-[13px] leading-relaxed">{current.description}</p>}

              {!current.approved && (
                <Button variant="outline" size="sm" onClick={() => setApproving(current)} className="w-fit text-working">
                  Review access to turn it on
                </Button>
              )}

              {(currentUpdate || current.source?.kind === "github" || !current.dev) && (
                <div className="flex flex-wrap gap-2">
                  {currentUpdate && (
                    <Button variant="secondary" size="sm" disabled={pending} onClick={() => void reinstall(current)}>
                      {pending ? <Spinner /> : <CircleArrowUpIcon aria-hidden />}
                      Update to {currentUpdate}
                    </Button>
                  )}
                  {current.source?.kind === "github" && !current.dev && (
                    <Button variant="outline" size="sm" disabled={pending} onClick={() => void reinstall(current)}>
                      <RefreshCwIcon aria-hidden />
                      Reinstall from GitHub
                    </Button>
                  )}
                  {!current.dev && (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={pending}
                      className="text-destructive hover:text-destructive"
                      onClick={() =>
                        void work(current.id, async () => {
                          await api.pluginRemove(current.id);
                          await refresh();
                        })
                      }
                    >
                      <TrashIcon aria-hidden />
                      Remove
                    </Button>
                  )}
                </div>
              )}
              {rowError[current.id] && (
                <p role="alert" className="text-[12px] text-blocked">
                  {rowError[current.id]}
                </p>
              )}

              <section className="flex flex-col gap-2">
                <h5 className="text-[13px] font-semibold">What it can access</h5>
                <Permissions plugin={current} />
              </section>

              <PluginNeeds plugin={current} />

              {/* Adds renders nothing for a plugin that adds nothing, and the heading goes with it. */}
              <section className="hidden flex-col gap-1 has-[ul]:flex">
                <h5 className="text-[13px] font-semibold">What it adds</h5>
                <Adds plugin={current} />
              </section>

              {currentReadme && (
                <section className="flex flex-col gap-2 border-t border-hairline pt-4" data-testid="plugin-readme">
                  <h5 className="text-[13px] font-semibold">README</h5>
                  <Markdown source={currentReadme} />
                </section>
              )}
            </motion.div>
          )}
        </ScrollArea>
      </div>
      <Approve plugin={approving} busy={approving ? busy.has(approving.id) : false} onCancel={() => setApproving(null)} onApprove={() => void approve()} />
    </div>
  );
}
