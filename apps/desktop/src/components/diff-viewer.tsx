import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { parsePatchFiles, type DiffLineAnnotation, type FileDiffMetadata } from "@pierre/diffs";
import { FileDiff } from "@pierre/diffs/react";
import { AnimatePresence, motion } from "motion/react";
import {
  ChevronRightIcon,
  CircleAlertIcon,
  Columns2Icon,
  ExternalLinkIcon,
  ListIcon,
  ListTreeIcon,
  MessageSquareIcon,
  Rows3Icon,
  SearchIcon,
} from "lucide-react";

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Item } from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { DiffComment, DiffView } from "@/lib/plugins";
import { cn } from "@/lib/utils";

type Entry = { file: FileDiffMetadata; added: number; removed: number };
type Thread = { root: DiffComment; replies: DiffComment[] };

const changeMark: Record<FileDiffMetadata["type"], { letter: string; className: string }> = {
  new: { letter: "A", className: "text-done" },
  deleted: { letter: "D", className: "text-blocked" },
  change: { letter: "M", className: "text-working" },
  "rename-pure": { letter: "R", className: "text-[#9cc0ff]" },
  "rename-changed": { letter: "R", className: "text-[#9cc0ff]" },
};

function entries(patch: string): Entry[] {
  return parsePatchFiles(patch).flatMap((p) =>
    p.files.map((file) => ({
      file,
      added: file.hunks.reduce((n, h) => n + h.additionLines, 0),
      removed: file.hunks.reduce((n, h) => n + h.deletionLines, 0),
    })),
  );
}

type Folder = { name: string; path: string; folders: Map<string, Folder>; files: Entry[] };
type TreeRow = { kind: "folder"; folder: Folder; label: string; depth: number } | { kind: "file"; entry: Entry; depth: number };

function buildTree(list: Entry[]): Folder {
  const root: Folder = { name: "", path: "", folders: new Map(), files: [] };
  for (const entry of list) {
    let node = root;
    for (const part of entry.file.name.split("/").slice(0, -1)) {
      let next = node.folders.get(part);
      if (!next) {
        next = { name: part, path: node.path ? `${node.path}/${part}` : part, folders: new Map(), files: [] };
        node.folders.set(part, next);
      }
      node = next;
    }
    node.files.push(entry);
  }
  return root;
}

/** Folders first, then files, by name. A folder holding only one folder joins it on one row (`src/lib`). */
function treeRows(node: Folder, collapsed: Set<string>, depth = 0, rows: TreeRow[] = []): TreeRow[] {
  for (const top of [...node.folders.values()].sort((a, b) => a.name.localeCompare(b.name))) {
    let folder = top;
    let label = top.name;
    while (folder.files.length === 0 && folder.folders.size === 1) {
      folder = [...folder.folders.values()][0];
      label += `/${folder.name}`;
    }
    rows.push({ kind: "folder", folder, label, depth });
    if (!collapsed.has(folder.path)) treeRows(folder, collapsed, depth + 1, rows);
  }
  const files = [...node.files].sort((a, b) => a.file.name.localeCompare(b.file.name));
  for (const entry of files) rows.push({ kind: "file", entry, depth });
  return rows;
}

const TREE_KEY = "wings.diff.tree";
function savedTree() {
  try {
    return localStorage.getItem(TREE_KEY) === "1";
  } catch {
    return false;
  }
}

/** Groups replies under the comment they answer, oldest first, by file path (null for the whole PR). */
function threadsByPath(comments: DiffComment[] = []) {
  const ids = new Set(comments.map((c) => c.id));
  const byPath = new Map<string | null, Thread[]>();
  const roots = new Map<number, Thread>();
  const sorted = [...comments].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  for (const c of sorted) {
    if (c.replyTo !== null && ids.has(c.replyTo)) continue;
    const thread = { root: c, replies: [] };
    roots.set(c.id, thread);
    byPath.set(c.path, [...(byPath.get(c.path) ?? []), thread]);
  }
  for (const c of sorted) if (c.replyTo !== null) roots.get(c.replyTo)?.replies.push(c);
  return byPath;
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
function ago(iso: string) {
  const days = Math.round((Date.parse(iso) - Date.now()) / 86_400_000);
  if (Math.abs(days) >= 1) return relative.format(days, "day");
  const hours = Math.round((Date.parse(iso) - Date.now()) / 3_600_000);
  return Math.abs(hours) >= 1 ? relative.format(hours, "hour") : relative.format(Math.round((Date.parse(iso) - Date.now()) / 60_000), "minute");
}

/** Comment links come from the plugin, so they go through its `openUrl` permission like any other link it opens. */
function CommentBody({ comment, onOpen }: { comment: DiffComment; onOpen: (url: string) => void }) {
  return (
    <div className="flex gap-2.5">
      <Avatar size="sm" className="mt-px">
        <AvatarFallback className="text-[11px] font-semibold uppercase">{comment.author.slice(0, 1)}</AvatarFallback>
      </Avatar>
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-2 text-[12px]">
          <span className="font-semibold">{comment.author}</span>
          <span className="text-muted-foreground">{ago(comment.createdAt)}</span>
          {comment.url && (
            <Button variant="ghost" size="icon-xs" onClick={() => onOpen(comment.url)} aria-label="Open comment in the browser" className="ml-auto text-muted-foreground">
              <ExternalLinkIcon />
            </Button>
          )}
        </p>
        <p className="mt-1 text-[13px] leading-relaxed break-words whitespace-pre-wrap select-text">{comment.body}</p>
      </div>
    </div>
  );
}

function CommentThread({ thread, className, onOpen }: { thread: Thread; className?: string; onOpen: (url: string) => void }) {
  return (
    <div
      className={cn(
        "max-w-3xl rounded-xl border border-hairline bg-[#1d1b22] p-3 font-sans shadow-[0_6px_20px_rgb(0_0_0/0.25)]",
        className ?? "my-1.5 mr-3 ml-12",
      )}
    >
      {thread.root.path !== null && className && <p className="mb-2.5 truncate text-[11px] text-muted-foreground">{thread.root.path}</p>}
      <CommentBody comment={thread.root} onOpen={onOpen} />
      {thread.replies.length > 0 && (
        <div className="mt-3 flex flex-col gap-3 border-l border-hairline-strong pl-3">
          {thread.replies.map((reply) => (
            <CommentBody key={reply.id} comment={reply} onOpen={onOpen} />
          ))}
        </div>
      )}
    </div>
  );
}

function LoadingDiff() {
  return (
    <div className="flex min-h-0 flex-1" aria-hidden>
      <div className="flex w-72 shrink-0 flex-col gap-2.5 border-r border-hairline p-3">
        <Skeleton className="h-8 bg-white/[0.05]" />
        {Array.from({ length: 9 }, (_, i) => (
          <Skeleton key={i} className="h-4 bg-white/[0.05]" style={{ width: `${55 + ((i * 37) % 40)}%` }} />
        ))}
      </div>
      <div className="flex flex-1 flex-col gap-2 p-5">
        {Array.from({ length: 18 }, (_, i) => (
          <Skeleton key={i} className="h-3.5 bg-white/[0.04]" style={{ width: `${30 + ((i * 53) % 60)}%` }} />
        ))}
      </div>
    </div>
  );
}

/** Full-window diff: file list on the left, the selected file with its review comments on the right. */
export function DiffViewer({ diff, onClose }: { diff: DiffView | null; onClose: () => void }) {
  const files = useMemo(() => (diff?.patch ? entries(diff.patch) : []), [diff?.patch]);
  const threads = useMemo(() => threadsByPath(diff?.comments), [diff?.comments]);
  /** A file index, or null for the discussion. */
  const [selected, setSelected] = useState<number | null>(0);
  const [split, setSplit] = useState(true);
  const [query, setQuery] = useState("");
  const [tree, setTree] = useState(savedTree);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const loading = !!diff && diff.patch === undefined && !diff.error;
  const shown = useMemo(() => files.filter((e) => e.file.name.toLowerCase().includes(query.trim().toLowerCase())), [files, query]);
  const rows = useMemo(() => (tree ? treeRows(buildTree(shown), collapsed) : []), [tree, shown, collapsed]);
  const current = selected === null ? undefined : files[selected];
  // Comments on the whole PR, then ones on code that has changed since, which have no line to sit on.
  const discussion = [...(threads.get(null) ?? []), ...[...threads].flatMap(([path, list]) => (path === null ? [] : list.filter((t) => t.root.line === null)))];
  const discussionCount = discussion.reduce((n, t) => n + 1 + t.replies.length, 0);
  const totals = files.reduce((t, e) => ({ added: t.added + e.added, removed: t.removed + e.removed }), { added: 0, removed: 0 });
  const commentCount = diff?.comments?.length ?? 0;
  const outdated = diff?.comments?.filter((c) => c.line === null && c.replyTo === null).length ?? 0;
  const annotations: DiffLineAnnotation<Thread>[] = (current ? (threads.get(current.file.name) ?? []) : [])
    .filter((t) => t.root.line !== null)
    .map((t) => ({ side: t.root.side, lineNumber: t.root.line!, metadata: t }));

  const openComment = (url: string) => {
    if (diff) void invoke("plugin_open_url", { pluginId: diff.pluginId, url }).catch((e) => console.error(e));
  };

  const toggleFolder = (path: string) =>
    setCollapsed((all) => {
      const next = new Set(all);
      if (!next.delete(path)) next.add(path);
      return next;
    });

  const chooseTree = (value: boolean) => {
    setTree(value);
    try {
      localStorage.setItem(TREE_KEY, value ? "1" : "0");
    } catch {
      // The choice just won't be remembered.
    }
  };

  /** In the tree, rows are indented by `depth` and show only the file name; the flat list adds the folder. */
  const fileRow = (entry: Entry, depth?: number) => {
    const index = files.indexOf(entry);
    const slash = entry.file.name.lastIndexOf("/");
    const mark = changeMark[entry.file.type];
    const fileComments = threads.get(entry.file.name)?.reduce((n, t) => n + 1 + t.replies.length, 0) ?? 0;
    return (
      <li key={entry.file.name}>
        <Item
          size="xs"
          render={<button type="button" />}
          onClick={() => setSelected(index)}
          aria-current={index === selected ? "true" : undefined}
          title={entry.file.name}
          style={depth === undefined ? undefined : { paddingLeft: 8 + depth * 12 + 18 }}
          className={cn("flex-nowrap rounded-md px-2 py-1.5 text-left text-[12px]", index === selected ? "bg-white/[0.08]" : "hover:bg-hover")}
        >
          <span className={cn("w-3 shrink-0 font-semibold", mark.className)}>{mark.letter}</span>
          <span className="min-w-0 flex-1 truncate">
            <span className="text-foreground">{entry.file.name.slice(slash + 1)}</span>
            {depth === undefined && slash > 0 && <span className="ml-1.5 text-muted-foreground">{entry.file.name.slice(0, slash)}</span>}
          </span>
          {fileComments > 0 && (
            <span className="flex shrink-0 items-center gap-0.5 text-muted-foreground" aria-label={`${fileComments} comments`}>
              <MessageSquareIcon className="size-3" aria-hidden />
              {fileComments}
            </span>
          )}
          <span className="shrink-0 tabular-nums text-done">+{entry.added}</span>
          <span className="shrink-0 tabular-nums text-blocked">−{entry.removed}</span>
        </Item>
      </li>
    );
  };

  return (
    <Dialog
      open={diff !== null}
      onOpenChange={(open) => {
        if (open) return;
        onClose();
        setSelected(0);
        setQuery("");
        setCollapsed(new Set());
      }}
    >
      <DialogContent className="flex h-[88vh] w-[min(1440px,94vw)] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none">
        <header className="relative flex shrink-0 items-center gap-4 border-b border-hairline py-3 pr-14 pl-5">
          <div className="min-w-0 flex-1">
            <DialogTitle className="truncate text-[15px] font-semibold">{diff?.title}</DialogTitle>
            <DialogDescription className="mt-0.5 flex items-center gap-2 text-[12px] text-muted-foreground">
              {diff?.subtitle && <span className="truncate">{diff.subtitle}</span>}
              {loading ? (
                <span className="flex items-center gap-1.5">
                  <Spinner className="size-3.5" />
                  Loading changes…
                </span>
              ) : (
                diff?.patch !== undefined && (
                  <>
                    <span>{files.length} files</span>
                    <span className="text-done">+{totals.added}</span>
                    <span className="text-blocked">−{totals.removed}</span>
                    {commentCount > 0 && (
                      <span className="flex items-center gap-1">
                        <MessageSquareIcon className="size-3.5" aria-hidden />
                        {commentCount} comments{outdated > 0 && `, ${outdated} on code that changed since`}
                      </span>
                    )}
                  </>
                )
              )}
            </DialogDescription>
          </div>
          <ToggleGroup
            aria-label="Diff layout"
            size="sm"
            value={[split ? "split" : "unified"]}
            onValueChange={(value) => value[0] && setSplit(value[0] === "split")}
            className="rounded-lg bg-white/[0.05] p-0.5"
          >
            <ToggleGroupItem value="split" className="gap-1.5 text-[12px] data-pressed:bg-white/[0.09]">
              <Columns2Icon aria-hidden />
              Split
            </ToggleGroupItem>
            <ToggleGroupItem value="unified" className="gap-1.5 text-[12px] data-pressed:bg-white/[0.09]">
              <Rows3Icon aria-hidden />
              Unified
            </ToggleGroupItem>
          </ToggleGroup>
          {loading && (
            <Progress value={null} aria-label="Loading changes" className="absolute inset-x-0 bottom-0 gap-0 [&_[data-slot=progress-track]]:h-px [&_[data-slot=progress-track]]:bg-transparent" />
          )}
        </header>

        <AnimatePresence mode="wait" initial={false}>
          {loading ? (
            <motion.div key="loading" className="flex min-h-0 flex-1" exit={{ opacity: 0 }} transition={{ duration: 0.15 }}>
              <LoadingDiff />
            </motion.div>
          ) : diff?.error ? (
            <motion.div key="error" initial={{ opacity: 0 }} animate={{ opacity: 1 }} role="alert" className="flex flex-1">
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon" className="text-blocked">
                    <CircleAlertIcon />
                  </EmptyMedia>
                  <EmptyTitle>Couldn't load the changes</EmptyTitle>
                  <EmptyDescription>{diff.error}</EmptyDescription>
                </EmptyHeader>
              </Empty>
            </motion.div>
          ) : (
            <motion.div
              key="content"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.2 }}
              className="flex min-h-0 flex-1"
            >
              <aside className="flex w-72 shrink-0 flex-col border-r border-hairline">
                <div className="flex items-center gap-1.5 p-2">
                  <InputGroup className="h-8 min-w-0 flex-1">
                    <InputGroupAddon>
                      <SearchIcon aria-hidden />
                    </InputGroupAddon>
                    <InputGroupInput value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filter files" aria-label="Filter files" className="text-[12px]" />
                  </InputGroup>
                  <ToggleGroup
                    aria-label="File list layout"
                    size="sm"
                    value={[tree ? "tree" : "flat"]}
                    onValueChange={(value) => value[0] && chooseTree(value[0] === "tree")}
                    className="shrink-0 rounded-md bg-white/[0.05] p-0.5"
                  >
                    <ToggleGroupItem value="flat" aria-label="Flat list" title="Flat list" className="data-pressed:bg-white/[0.09]">
                      <ListIcon aria-hidden />
                    </ToggleGroupItem>
                    <ToggleGroupItem value="tree" aria-label="Folder tree" title="Folder tree" className="data-pressed:bg-white/[0.09]">
                      <ListTreeIcon aria-hidden />
                    </ToggleGroupItem>
                  </ToggleGroup>
                </div>
                <ScrollArea className="min-h-0 flex-1">
                  <ul className="flex flex-col gap-px px-2 pb-2">
                    {discussionCount > 0 && (
                      <li className="mb-1 border-b border-hairline pb-1">
                        <Item
                          size="xs"
                          render={<button type="button" />}
                          onClick={() => setSelected(null)}
                          aria-current={selected === null ? "true" : undefined}
                          className={cn("flex-nowrap rounded-md px-2 py-1.5 text-left text-[12px]", selected === null ? "bg-white/[0.08]" : "hover:bg-hover")}
                        >
                          <MessageSquareIcon className="size-3 shrink-0 text-muted-foreground" aria-hidden />
                          <span className="flex-1 font-medium">Discussion</span>
                          <span className="tabular-nums text-muted-foreground" aria-label={`${discussionCount} comments`}>
                            {discussionCount}
                          </span>
                        </Item>
                      </li>
                    )}
                    {tree
                      ? rows.map((row) =>
                          row.kind === "file" ? (
                            fileRow(row.entry, row.depth)
                          ) : (
                            <li key={`dir:${row.folder.path}`}>
                              <Item
                                size="xs"
                                render={<button type="button" />}
                                onClick={() => toggleFolder(row.folder.path)}
                                aria-expanded={!collapsed.has(row.folder.path)}
                                title={row.folder.path}
                                style={{ paddingLeft: 8 + row.depth * 12 }}
                                className="flex-nowrap gap-1.5 rounded-md py-1.5 pr-2 text-left text-[12px] text-muted-foreground hover:bg-hover hover:text-foreground"
                              >
                                <ChevronRightIcon
                                  className={cn("size-3 shrink-0 transition-transform duration-150", !collapsed.has(row.folder.path) && "rotate-90")}
                                  aria-hidden
                                />
                                <span className="min-w-0 truncate">{row.label}</span>
                              </Item>
                            </li>
                          ),
                        )
                      : shown.map((entry) => fileRow(entry))}
                  </ul>
                </ScrollArea>
              </aside>

              <section className="min-w-0 flex-1 overflow-auto bg-[#141217]">
                {selected === null ? (
                  <div className="flex flex-col gap-3 p-5">
                    {discussion.map((thread) => (
                      <CommentThread key={thread.root.id} thread={thread} className="w-full" onOpen={openComment} />
                    ))}
                  </div>
                ) : current ? (
                  <FileDiff<Thread>
                    key={`${current.file.name}-${split}`}
                    fileDiff={current.file}
                    lineAnnotations={annotations}
                    renderAnnotation={(a) => <CommentThread thread={a.metadata} onOpen={openComment} />}
                    options={{ theme: "pierre-dark", themeType: "dark", diffStyle: split ? "split" : "unified", stickyHeader: true }}
                  />
                ) : (
                  <Empty>
                    <EmptyHeader>
                      <EmptyTitle>No file changes</EmptyTitle>
                      <EmptyDescription>This pull request doesn't change any files.</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                )}
              </section>
            </motion.div>
          )}
        </AnimatePresence>
      </DialogContent>
    </Dialog>
  );
}
