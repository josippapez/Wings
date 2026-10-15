import { useEffect, useMemo, useRef, useState } from "react";
import type { SelectedLineRange } from "@pierre/diffs";
import { invoke } from "@tauri-apps/api/core";
import { parsePatchFiles, type DiffLineAnnotation, type FileDiffMetadata } from "@pierre/diffs";
import { getOrCreateWorkerPoolSingleton, type WorkerPoolManager } from "@pierre/diffs/worker";
import { FileDiff, useVirtualizer, Virtualizer, WorkerPoolContext, type FileDiffOptions } from "@pierre/diffs/react";
import DiffWorker from "@pierre/diffs/worker/worker.js?worker";
import { AnimatePresence, motion } from "motion/react";
import {
  ChevronRightIcon,
  CircleAlertIcon,
  Columns2Icon,
  ExternalLinkIcon,
  ListIcon,
  ListTreeIcon,
  MessageSquareIcon,
  ReplyIcon,
  Rows3Icon,
  SearchIcon,
  TextWrapIcon,
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
import { Textarea } from "@/components/ui/textarea";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { DiffScrollbars } from "@/components/diff-scrollbars";
import { ReviewDraft, ReviewNote, ReviewPanel } from "@/components/review-panel";
import { LINE_HEIGHT, rowNumber, rowTop } from "@/lib/diff-scroll";
import { resolveFocus, type ResolvedFocus } from "@/lib/diff-focus";
import type { DiffComment, DiffView } from "@/lib/plugins";
import { quoteLines, type ReviewComment, type ReviewTarget } from "@/lib/review";
import { cn } from "@/lib/utils";

type Entry = { file: FileDiffMetadata; added: number; removed: number };
type Thread = { root: DiffComment; replies: DiffComment[] };
/** What sits under a line: a plugin's comment thread, a comment of yours that isn't sent yet, or the box you're typing one in. */
type Note = { kind: "thread"; thread: Thread } | { kind: "review"; comment: ReviewComment } | { kind: "draft"; range: SelectedLineRange };

/** No unsent comments, as one array, so the annotations built from it keep their identity. */
const NO_REVIEW: ReviewComment[] = [];

const rangeLabel = (r: SelectedLineRange) => (r.start === r.end ? `line ${r.start}` : `lines ${Math.min(r.start, r.end)} to ${Math.max(r.start, r.end)}`);

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

const WRAP_KEY = "wings.diff.wrap";
function savedWrap() {
  try {
    return localStorage.getItem(WRAP_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * Shift and a mouse wheel scroll the code under the pointer sideways. Unwrapped code scrolls inside each of the library's code columns, whose
 * scrollbar sits at the bottom of the file, out of reach. A wheel event that already moves sideways (a trackpad) is left to the browser.
 */
function onShiftWheel(e: WheelEvent) {
  if (!e.shiftKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
  const code = e.composedPath().find((el): el is HTMLElement => el instanceof HTMLElement && el.hasAttribute("data-code"));
  if (!code || code.scrollWidth <= code.clientWidth) return;
  e.preventDefault();
  code.scrollLeft += e.deltaY;
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

/** Sends with ⌘↩ or the button. A failed send keeps the text and shows the plugin's error. */
function ReplyBox({ onSend }: { onSend: (body: string) => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!open) {
    return (
      <Button variant="ghost" size="xs" onClick={() => setOpen(true)} className="mt-2.5 -ml-2 text-muted-foreground">
        <ReplyIcon aria-hidden />
        Reply
      </Button>
    );
  }
  const send = async () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSend(text.trim());
      setText("");
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mt-3 flex flex-col gap-2">
      <Textarea
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void send();
          }
        }}
        readOnly={busy}
        placeholder="Write a reply"
        aria-label="Reply"
        aria-invalid={error ? true : undefined}
        className="min-h-16 text-[13px]"
      />
      {error && (
        <p role="alert" className="text-[12px] text-blocked">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-1.5">
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => (setOpen(false), setError(null))}>
          Cancel
        </Button>
        <Button size="sm" disabled={busy || !text.trim()} onClick={() => void send()}>
          {busy && <Spinner className="size-3.5" />}
          Reply
        </Button>
      </div>
    </div>
  );
}

/**
 * Brings the line a plugin pointed at (`focus`) to the middle of the diff. A big file draws only the lines near the viewport, so a far line
 * has no element yet: scroll toward it from whichever row is on screen until it shows, then centre it. The distance comes from the hunks (rows and
 * the separators between them), since a big diff has far fewer rows than lines. It aims 15 rows above the line so the line lands inside the drawn
 * window, not on its edge, and waits for the window to move before it steps again so it can't overshoot.
 */
function ScrollToLine({ file, line, side }: { file: FileDiffMetadata | undefined; line: number | undefined; side: "additions" | "deletions" }) {
  const virtualizer = useVirtualizer();
  useEffect(() => {
    const row = file && line !== undefined ? rowNumber(file, side, line) : null;
    if (!row) return;
    let frame = 0;
    let tries = 0;
    let stepped: { from: number; frames: number } | null = null;
    const tick = () => {
      const root = document.querySelector("diffs-container")?.shadowRoot;
      const unified = root?.querySelector("code[data-unified]");
      const column = unified ?? root?.querySelector(side === "deletions" ? "code[data-deletions]" : "code[data-additions]");
      // Split columns number rows by their own side. The unified column has one numbering, which rowNumber maps to.
      const want = unified ? row.number : line!;
      const rows = [...(column?.querySelectorAll("[data-line]") ?? [])].filter((el) => !unified || (el.getAttribute("data-line-type") === "change-deletion") === row.deleted);
      const hit = rows.find((el) => Number(el.getAttribute("data-line")) === want);
      if (hit) return hit.scrollIntoView({ block: "center" });
      const seen = rows[0] ? Number(rows[0].getAttribute("data-line")) : 0;
      if (seen && virtualizer && !(stepped && stepped.from === seen && ++stepped.frames < 30)) {
        const split = !unified;
        const from = rowTop(file!, split ? side : row.deleted ? "deletions" : "additions", seen, split);
        const to = rowTop(file!, split ? side : row.deleted ? "deletions" : "additions", want, split);
        if (from !== null && to !== null) virtualizer.scrollTo({ top: virtualizer.getScrollTop() + to - from - 15 * LINE_HEIGHT });
        stepped = { from: seen, frames: 0 };
      }
      if (++tries < 240) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [file, line, side, virtualizer]);
  return null;
}

function CommentThread({
  thread,
  className,
  focused,
  onOpen,
  onReply,
}: {
  thread: Thread;
  className?: string;
  /** The plugin pointed at this thread: ring it while the highlight lasts. */
  focused?: boolean;
  onOpen: (url: string) => void;
  onReply?: (replyTo: number, body: string) => Promise<void>;
}) {
  return (
    <div
      className={cn(
        "max-w-3xl rounded-xl border border-hairline bg-[#1d1b22] p-3 font-sans shadow-[0_6px_20px_rgb(0_0_0/0.25)] transition-shadow duration-500",
        focused && "ring-2 ring-white/40",
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
      {onReply && thread.root.canReply && <ReplyBox onSend={(body) => onReply(thread.root.id, body)} />}
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
export function DiffViewer({
  diff,
  onClose,
  onReply,
  reviewTargets,
  onSendReview,
}: {
  diff: DiffView | null;
  onClose: () => void;
  onReply: (diff: DiffView, replyTo: number, body: string) => Promise<void>;
  /** The panes your review can go to, asked when you choose one. `cwd` is the repo the diff is of, when the plugin says. */
  reviewTargets: (cwd: string | undefined) => { targets: ReviewTarget[]; preferred: string | null };
  /** Pastes the review into the pane; presses Enter after when `submit`. */
  onSendReview: (paneId: string, text: string, submit: boolean) => Promise<void>;
}) {
  // The highlighter workers start the first time a diff opens (while its patch still loads) and then stay, so a later open doesn't pay for the WASM and Shiki set-up again.
  const [pool, setPool] = useState<WorkerPoolManager>();
  if (diff && !pool) setPool(getOrCreateWorkerPoolSingleton({ poolOptions: { workerFactory: () => new DiffWorker(), poolSize: 2 }, highlighterOptions: { theme: "pierre-dark" } }));
  const files = useMemo(() => (diff?.patch ? entries(diff.patch) : []), [diff?.patch]);
  const threads = useMemo(() => threadsByPath(diff?.comments), [diff?.comments]);
  /** A file index, or null for the discussion. */
  const [selected, setSelected] = useState<number | null>(0);
  const [split, setSplit] = useState(true);
  const [wrap, setWrap] = useState(savedWrap);
  const chooseWrap = (value: boolean) => {
    setWrap(value);
    try {
      localStorage.setItem(WRAP_KEY, value ? "1" : "0");
    } catch {
      // The choice just won't be remembered.
    }
  };
  const wheelArea = useRef<HTMLDivElement>(null);
  // Not React's onWheel: React listens passively, so it couldn't stop the page scrolling down as well.
  useEffect(() => {
    const el = wheelArea.current;
    if (!el) return;
    el.addEventListener("wheel", onShiftWheel, { passive: false });
    return () => el.removeEventListener("wheel", onShiftWheel);
  });
  const [query, setQuery] = useState("");
  const [tree, setTree] = useState(savedTree);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  /** Your unsent comments, kept for the diff they were written on. */
  const [review, setReview] = useState<{ id: string | null; items: ReviewComment[] }>({ id: null, items: [] });
  const [draft, setDraft] = useState<{ path: string; range: SelectedLineRange } | null>(null);
  /** The line a plugin pointed at, highlighted for a moment. */
  const [flash, setFlash] = useState<(ResolvedFocus & { path: string; done?: boolean }) | null>(null);
  const focused = useRef<DiffView["focus"]>(undefined);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const reviewItems = review.id === diff?.id ? review.items : NO_REVIEW;
  const loading = !!diff && diff.patch === undefined && !diff.error;
  const shown = useMemo(() => files.filter((e) => e.file.name.toLowerCase().includes(query.trim().toLowerCase())), [files, query]);
  const rows = useMemo(() => (tree ? treeRows(buildTree(shown), collapsed) : []), [tree, shown, collapsed]);
  const current = selected === null ? undefined : files[selected];
  // Once the patch is here, open the file the plugin pointed at and flash its line. Each `focus` is acted on once,
  // so a later `updateDiff` with only comments leaves where you are alone.
  useEffect(() => {
    const focus = diff?.focus;
    if (!focus || diff?.patch === undefined || focused.current === focus) return;
    focused.current = focus;
    const hit = resolveFocus(files.map((e) => e.file), focus);
    if (!hit) return;
    setSelected(hit.index);
    clearTimeout(flashTimer.current);
    if (hit.line === undefined) return setFlash(null);
    setFlash({ ...hit, path: files[hit.index].file.name });
    // First clear the selection (null), then drop the prop: a library selection that was set from outside stays until told otherwise.
    flashTimer.current = setTimeout(() => {
      setFlash((f) => f && { ...f, done: true });
      flashTimer.current = setTimeout(() => setFlash(null), 200);
    }, 2500);
  }, [diff?.focus, diff?.patch, files]);
  useEffect(() => () => clearTimeout(flashTimer.current), []);
  // Comments on the whole PR, then ones on code that has changed since, which have no line to sit on.
  // The thread with the newest comment first, like the PR tab.
  const discussion = [...(threads.get(null) ?? []), ...[...threads].flatMap(([path, list]) => (path === null ? [] : list.filter((t) => t.root.line === null)))].sort((a, b) =>
    (b.replies.at(-1) ?? b.root).createdAt.localeCompare((a.replies.at(-1) ?? a.root).createdAt),
  );
  const discussionCount = discussion.reduce((n, t) => n + 1 + t.replies.length, 0);
  const totals = files.reduce((t, e) => ({ added: t.added + e.added, removed: t.removed + e.removed }), { added: 0, removed: 0 });
  const commentCount = diff?.comments?.length ?? 0;
  const outdated = diff?.comments?.filter((c) => c.line === null && c.replyTo === null).length ?? 0;
  // The file's annotations and options keep their identity until what they show changes. @pierre/diffs compares both
  // by identity (options key by key), so a new one, on every render of App, redrew every row of the open file.
  const annotations = useMemo<DiffLineAnnotation<Note>[]>(
    () =>
      current
        ? [
            ...(threads.get(current.file.name) ?? [])
              .filter((t) => t.root.line !== null)
              .map((t): DiffLineAnnotation<Note> => ({ side: t.root.side, lineNumber: t.root.line!, metadata: { kind: "thread", thread: t } })),
            ...reviewItems
              .filter((c) => c.path === current.file.name)
              .map((c): DiffLineAnnotation<Note> => ({ side: c.side, lineNumber: Math.max(c.start, c.end), metadata: { kind: "review", comment: c } })),
            ...(draft && draft.path === current.file.name
              ? [{ side: draft.range.endSide ?? draft.range.side ?? "additions", lineNumber: Math.max(draft.range.start, draft.range.end), metadata: { kind: "draft", range: draft.range } } satisfies DiffLineAnnotation<Note>]
              : []),
          ]
        : [],
    [current, threads, reviewItems, draft],
  );
  const currentName = current?.file.name;
  const options = useMemo<FileDiffOptions<Note, undefined>>(
    () => ({
      theme: "pierre-dark",
      themeType: "dark",
      diffStyle: split ? "split" : "unified",
      overflow: wrap ? "wrap" : "scroll",
      stickyHeader: true,
      // The + that shows beside a line number: click it for one line, drag it for several.
      enableGutterUtility: true,
      onGutterUtilityClick: (range) => {
        if (currentName !== undefined) setDraft({ path: currentName, range });
      },
    }),
    [split, wrap, currentName],
  );

  const addReview = (path: string, range: SelectedLineRange, body: string) => {
    const side = range.side ?? "additions";
    const file = files.find((e) => e.file.name === path)?.file;
    const comment: ReviewComment = { id: crypto.randomUUID(), path, side, start: range.start, end: range.end, quote: file ? quoteLines(file, side, range.start, range.end) : [], body };
    setReview({ id: diff?.id ?? null, items: [...reviewItems, comment] });
    setDraft(null);
  };
  const removeReview = (id: string) => setReview({ id: diff?.id ?? null, items: reviewItems.filter((c) => c.id !== id) });

  const openComment = (url: string) => {
    if (diff) void invoke("plugin_open_url", { pluginId: diff.pluginId, url }).catch((e) => console.error(e));
  };
  const reply = diff ? (replyTo: number, body: string) => onReply(diff, replyTo, body) : undefined;

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
    <WorkerPoolContext.Provider value={pool}>
    <Dialog
      open={diff !== null}
      onOpenChange={(open) => {
        if (open) return;
        onClose();
        setDraft(null);
        setReview({ id: null, items: [] });
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
          <Toggle size="sm" aria-label="Wrap long lines" title="Wrap long lines. Unwrapped, Shift and the mouse wheel scroll sideways." pressed={wrap} onPressedChange={chooseWrap} className="gap-1.5 text-[12px] aria-pressed:bg-white/[0.09]">
            <TextWrapIcon aria-hidden />
            Wrap
          </Toggle>
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

              <div ref={wheelArea} className="relative flex min-w-0 flex-1">
              <Virtualizer className="min-w-0 flex-1 overflow-auto bg-[#141217]">
                <ScrollToLine file={flash ? files.find((e) => e.file.name === flash.path)?.file : undefined} line={flash?.line} side={flash?.side ?? "additions"} />
                {selected === null ? (
                  <div className="flex flex-col gap-3 p-5">
                    {discussion.map((thread) => (
                      <CommentThread key={thread.root.id} thread={thread} className="w-full" onOpen={openComment} onReply={reply} />
                    ))}
                  </div>
                ) : current ? (
                  <FileDiff<Note>
                    key={`${current.file.name}-${split}-${wrap}`}
                    fileDiff={current.file}
                    lineAnnotations={annotations}
                    // Undefined, not null, when nothing is flashing: a passed value makes the selection controlled, which would stop the gutter drag from highlighting.
                    selectedLines={flash?.path === current.file.name ? (flash.done ? null : { start: flash.line!, end: flash.line!, side: flash.side }) : undefined}
                    renderAnnotation={(a) =>
                      a.metadata.kind === "thread" ? (
                        <CommentThread
                          thread={a.metadata.thread}
                          focused={flash?.path === current.file.name && !flash.done && flash.line === a.lineNumber && flash.side === a.side}
                          onOpen={openComment}
                          onReply={reply}
                        />
                      ) : a.metadata.kind === "review" ? (
                        <ReviewNote comment={a.metadata.comment} onRemove={() => removeReview(a.metadata.kind === "review" ? a.metadata.comment.id : "")} />
                      ) : (
                        <ReviewDraft range={rangeLabel(a.metadata.range)} onAdd={(body) => addReview(current.file.name, a.metadata.kind === "draft" ? a.metadata.range : { start: 0, end: 0 }, body)} onCancel={() => setDraft(null)} />
                      )
                    }
                    options={options}
                  />
                ) : (
                  <Empty>
                    <EmptyHeader>
                      <EmptyTitle>No file changes</EmptyTitle>
                      <EmptyDescription>This pull request doesn't change any files.</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                )}
              </Virtualizer>
              {!wrap && <DiffScrollbars area={wheelArea} watch={`${currentName}-${split}`} />}
              </div>
              {reviewItems.length > 0 && (
                <ReviewPanel
                  comments={reviewItems}
                  title={diff?.title ?? ""}
                  targets={() => reviewTargets(diff?.cwd)}
                  onRemove={removeReview}
                  onJump={(c) => {
                    const at = files.findIndex((e) => e.file.name === c.path);
                    if (at !== -1) setSelected(at);
                  }}
                  onSend={async (paneId, text, submit) => {
                    await onSendReview(paneId, text, submit);
                    setReview({ id: diff?.id ?? null, items: [] });
                  }}
                />
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </DialogContent>
    </Dialog>
    </WorkerPoolContext.Provider>
  );
}
