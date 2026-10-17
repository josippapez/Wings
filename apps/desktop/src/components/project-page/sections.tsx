import { ArrowDownIcon, ArrowUpIcon, PencilIcon, SparklesIcon, Trash2Icon, UserIcon } from "lucide-react";
import { memo, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { IconButton } from "@/components/icon-button";
import { ErrorBoundary } from "@/components/error-boundary";
import { Markdown, type LocalImage } from "@/components/markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { LIMITS, editSection, projectInfo, type Section } from "@/lib/project-info";

import { ago } from "./ago";
import { useNow } from "./now";
import { imageExt, imageMarkdown, takeStored, type EditorState } from "./merge";
import { markClass, type Mark } from "./search";
import { messageOf, type Apply } from "./use-project-info";

/** Typing costs no render and no write: the text lives in the textarea, and a save goes out this long after typing stops, and on blur. */
const SAVE_MS = 600;

const same = (a: Section, b: Section) => a.id === b.id && a.title === b.title && a.body === b.body && a.by === b.by && a.updatedAt === b.updatedAt;

/** Re-renders only when its own section or position changes, so typing in one doesn't render the others. */
export const SectionCard = memo(
  function SectionCard({ projectId, section, first, last, startEditing, apply, read, mark }: { projectId: string; section: Section; first: boolean; last: boolean; startEditing: boolean; apply: Apply; read: LocalImage; mark?: Mark }) {
    const now = useNow();
    const [editing, setEditing] = useState(startEditing);
    const [confirm, setConfirm] = useState(false);
    const act = (op: Parameters<Apply>[0]) => apply(op).catch((e) => void toast.error(messageOf(e)));
    return (
      <article id={`item-${section.id}`} aria-label={section.title} className={cn("group/section rounded-xl border border-hairline bg-white/[0.02] px-5 py-4 transition-colors hover:border-hairline-strong focus-within:border-hairline-strong", markClass(mark))}>
        {editing ? (
          <SectionEditor projectId={projectId} section={section} apply={apply} onDone={() => setEditing(false)} />
        ) : (
          <>
            <header className="mb-2 flex min-h-8 items-center gap-1">
              <h3 className="min-w-0 flex-1 truncate text-[16px] font-semibold tracking-tight">{section.title}</h3>
              <span className="flex items-center opacity-0 transition-opacity group-focus-within/section:opacity-100 group-hover/section:opacity-100">
                <IconButton label={`Move ${section.title} up`} disabled={first} onClick={() => void act({ kind: "moveItem", id: section.id, dir: -1 })}>
                  <ArrowUpIcon />
                </IconButton>
                <IconButton label={`Move ${section.title} down`} disabled={last} onClick={() => void act({ kind: "moveItem", id: section.id, dir: 1 })}>
                  <ArrowDownIcon />
                </IconButton>
                <IconButton label={`Edit ${section.title}`} onClick={() => setEditing(true)}>
                  <PencilIcon />
                </IconButton>
                {confirm ? (
                  <>
                    <Button size="xs" variant="destructive" autoFocus onClick={() => void act({ kind: "deleteItem", id: section.id })}>
                      Delete section
                    </Button>
                    <Button size="xs" variant="ghost" onClick={() => setConfirm(false)}>
                      Keep
                    </Button>
                  </>
                ) : (
                  <IconButton label={`Delete ${section.title}`} onClick={() => setConfirm(true)}>
                    <Trash2Icon />
                  </IconButton>
                )}
              </span>
            </header>
            {section.body.trim() ? (
              <ErrorBoundary resetKey={section.body} fallback={<div className="text-[13px] text-muted-foreground"><p>This section couldn't be shown.</p><pre className="mt-1 whitespace-pre-wrap font-mono text-[12px]">{section.body}</pre></div>}>
                <Markdown source={section.body} read={read} />
              </ErrorBoundary>
            ) : <button type="button" onClick={() => setEditing(true)} className="-mx-1 rounded-md px-1 text-left text-[13px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">Empty. Press Edit to write.</button>}
            <p className="mt-3 flex items-center gap-1.5 text-[11px] text-muted-foreground">
              {section.by === "agent" ? <SparklesIcon className="size-3" aria-hidden /> : <UserIcon className="size-3" aria-hidden />}
              <span>{section.by === "agent" ? "Written by an agent" : "Written by you"}</span>
              <span aria-hidden>·</span>
              <time dateTime={new Date(section.updatedAt).toISOString()} title={new Date(section.updatedAt).toLocaleString()}>
                {ago(section.updatedAt, now)}
              </time>
            </p>
          </>
        )}
      </article>
    );
  },
  (a, b) => a.projectId === b.projectId && a.first === b.first && a.last === b.last && a.apply === b.apply && a.read === b.read && a.startEditing === b.startEditing && a.mark === b.mark && same(a.section, b.section),
);

function SectionEditor({ projectId, section, apply, onDone }: { projectId: string; section: Section; apply: Apply; onDone: () => void }) {
  const area = useRef<HTMLTextAreaElement>(null);
  const title = useRef<HTMLInputElement>(null);
  const editor = useRef<EditorState>({ base: section.body, text: section.body, dirty: false, inflight: false, sent: false, stale: "" });
  /** The title as typed: kept here, not read from the input, since a save sent after the editor closes has no input left. */
  const latestTitle = useRef(section.title);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [status, setStatus] = useState<"saved" | "unsaved" | "error">("saved");
  const [error, setError] = useState("");
  /** Counts finished saves, so a section that changed while one was out is looked at again. */
  const [settled, setSettled] = useState(0);

  // Takes what storage holds: an agent's append goes after what you typed (see `takeStored`).
  useEffect(() => {
    const el = area.current;
    if (!el) return;
    const took = takeStored(editor.current, section.body);
    if (!took) return;
    el.value = editor.current.text = took.text;
    editor.current.base = took.base;
  }, [section.body, settled]);

  const sectionRef = useRef(section);
  sectionRef.current = section;
  const send = useRef(() => {});
  send.current = () => {
    clearTimeout(timer.current);
    const s = editor.current;
    // One save at a time: a second would be merged against a base the first already changed.
    if (!s.dirty || s.inflight) return;
    s.dirty = false;
    s.inflight = true;
    s.sent = true;
    s.stale = sectionRef.current.body;
    const body = s.text;
    void apply(editSection(section.id, body, s.base, latestTitle.current)).then(
      () => {
        s.base = body;
        s.inflight = false;
        if (s.dirty) send.current();
        else setStatus("saved");
        setSettled((n) => n + 1);
      },
      (e) => {
        s.inflight = false;
        s.dirty = true;
        setError(messageOf(e));
        setStatus("error");
      },
    );
  };

  useEffect(() => {
    const flush = () => send.current();
    window.addEventListener("pagehide", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, []);

  const touched = () => {
    editor.current.dirty = true;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => send.current(), SAVE_MS);
    setStatus((s) => (s === "unsaved" ? s : "unsaved"));
  };

  /** Stores a pasted or dropped image and puts its Markdown at the cursor. */
  const addImage = async (file: File) => {
    const el = area.current;
    const ext = imageExt(file.type);
    if (!el) return;
    if (!ext) return void (setError("Only PNG, JPEG, GIF and WebP images can be added."), setStatus("error"));
    if (file.size > LIMITS.image) return void (setError(`An image is limited to ${LIMITS.image / 1024 / 1024} MB.`), setStatus("error"));
    const [start, end] = [el.selectionStart, el.selectionEnd];
    try {
      const stored = await projectInfo.addImage(projectId, new Uint8Array(await file.arrayBuffer()), ext);
      el.setRangeText(imageMarkdown(stored), start, end, "end");
      editor.current.text = el.value;
      touched();
    } catch (e) {
      setError(messageOf(e));
      setStatus("error");
    }
  };
  const imageIn = (files: FileList | null) => [...(files ?? [])].find((f) => f.type.startsWith("image/"));

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Input
          ref={title}
          aria-label="Section title"
          defaultValue={section.title}
          maxLength={LIMITS.title}
          onInput={(e) => {
            latestTitle.current = e.currentTarget.value;
            touched();
          }}
          onBlur={() => send.current()}
        />
        <Button size="sm" variant="secondary" onClick={() => (send.current(), onDone())}>
          Done
        </Button>
      </div>
      <textarea
        ref={area}
        aria-label={`${section.title}, Markdown`}
        placeholder="Markdown: lists, tables, [links](https://example.com). Paste or drop an image to add it."
        rows={12}
        maxLength={LIMITS.body}
        defaultValue={section.body}
        spellCheck
        autoFocus
        onInput={(e) => {
          editor.current.text = e.currentTarget.value;
          touched();
        }}
        onBlur={() => send.current()}
        onKeyDown={(e) => e.key === "Escape" && (send.current(), onDone())}
        onPaste={(e) => {
          const file = imageIn(e.clipboardData.files);
          if (!file) return;
          e.preventDefault();
          void addImage(file);
        }}
        onDragOver={(e) => imageIn(e.dataTransfer.files) && e.preventDefault()}
        onDrop={(e) => {
          const file = imageIn(e.dataTransfer.files);
          if (!file) return;
          e.preventDefault();
          void addImage(file);
        }}
        className="min-h-40 w-full resize-y rounded-lg border border-input bg-transparent p-2.5 font-mono text-[12.5px] leading-relaxed outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
      />
      <p role="status" className={status === "error" ? "text-[12px] text-destructive" : "text-[12px] text-muted-foreground"}>
        {status === "saved" ? "Saved" : status === "unsaved" ? "Saving..." : `Couldn't save: ${error} Your text is still here.`}
      </p>
    </div>
  );
}
