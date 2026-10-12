import { useEffect, useMemo, useRef, useState } from "react";
import { MessageSquareIcon, SendIcon, Trash2Icon } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { formatReview, type ReviewComment, type ReviewTarget } from "@/lib/review";
import { OPEN_REVIEW_SEND } from "@/lib/ui-events";

const lines = (c: ReviewComment) => (c.start === c.end ? `${c.start}` : `${Math.min(c.start, c.end)}-${Math.max(c.start, c.end)}`);
const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/** The box that opens under the lines you picked: type the comment, ⌘↩ adds it. */
export function ReviewDraft({ range, onAdd, onCancel }: { range: string; onAdd: (body: string) => void; onCancel: () => void }) {
  const [text, setText] = useState("");
  const add = () => text.trim() && onAdd(text.trim());
  return (
    <div className="my-1.5 mr-3 ml-12 flex max-w-3xl flex-col gap-2 rounded-xl border border-hairline bg-[#1d1b22] p-3 font-sans shadow-[0_6px_20px_rgb(0_0_0/0.25)]">
      <p className="text-[11px] text-muted-foreground">Comment on {range}</p>
      <Textarea
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            add();
          } else if (e.key === "Escape") {
            // Closes the comment, not the diff.
            e.stopPropagation();
            onCancel();
          }
        }}
        placeholder="What should the agent change?"
        aria-label="Review comment"
        className="min-h-16 text-[13px]"
      />
      <div className="flex justify-end gap-1.5">
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" disabled={!text.trim()} onClick={add}>
          Add comment
        </Button>
      </div>
    </div>
  );
}

/** A comment you wrote and haven't sent yet, shown under its lines. */
export function ReviewNote({ comment, onRemove }: { comment: ReviewComment; onRemove: () => void }) {
  return (
    <div className="my-1.5 mr-3 ml-12 flex max-w-3xl items-start gap-2 rounded-xl border border-working/30 bg-[#1d1b22] p-3 font-sans">
      <div className="min-w-0 flex-1">
        <p className="mb-1 text-[11px] text-working">Your comment, not sent yet</p>
        <p className="text-[13px] leading-relaxed break-words whitespace-pre-wrap select-text">{comment.body}</p>
      </div>
      <Button variant="ghost" size="icon-xs" onClick={onRemove} aria-label="Remove comment" className="text-muted-foreground">
        <Trash2Icon />
      </Button>
    </div>
  );
}

/** Your comments on the diff, and where to send them. Nothing goes to a pane until you press a send button. */
export function ReviewPanel({
  comments,
  title,
  targets,
  onRemove,
  onJump,
  onSend,
}: {
  comments: ReviewComment[];
  title: string;
  /** Asked when the panel opens and when the pane list does, so it's current without polling. */
  targets: () => { targets: ReviewTarget[]; preferred: string | null };
  onRemove: (id: string) => void;
  onJump: (comment: ReviewComment) => void;
  /** Pastes the text into the pane, and presses Enter after when `submit`. */
  onSend: (paneId: string, text: string, submit: boolean) => Promise<void>;
}) {
  const [found, setFound] = useState(targets);
  const [paneId, setPaneId] = useState<string | null>(found.preferred);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const refresh = () => {
    const next = targets();
    setFound(next);
    // A pane that closed can't stay chosen, and a lone agent in the repo is picked for you.
    setPaneId((id) => (id && next.targets.some((t) => t.paneId === id) ? id : next.preferred));
  };
  useEffect(() => {
    const listener = () => {
      refresh();
      trigger.current?.focus();
    };
    window.addEventListener(OPEN_REVIEW_SEND, listener);
    return () => window.removeEventListener(OPEN_REVIEW_SEND, listener);
  });

  const items = useMemo(() => found.targets.map((t) => ({ value: t.paneId, label: label(t) })), [found]);
  const agents = found.targets;

  const send = async (submit: boolean) => {
    if (!paneId || busy) return;
    const target = found.targets.find((t) => t.paneId === paneId);
    setBusy(true);
    setError(null);
    try {
      await onSend(paneId, formatReview(comments, title), submit);
      toast.success(`Sent ${comments.length === 1 ? "1 comment" : `${comments.length} comments`} to ${target ? label(target) : "the pane"}`, { duration: 2500 });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="flex w-80 shrink-0 flex-col border-l border-hairline" aria-label="Review comments">
      <h3 className="flex items-center gap-2 border-b border-hairline px-3 py-2.5 text-[13px] font-semibold">
        <MessageSquareIcon className="size-3.5 text-muted-foreground" aria-hidden />
        Your review
        <span className="ml-auto text-[12px] font-normal text-muted-foreground tabular-nums">{comments.length}</span>
      </h3>
      <ul className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-2">
        {comments.map((c) => (
          <li key={c.id} className="group/note rounded-lg p-2 hover:bg-hover">
            <div className="flex items-center gap-1">
              <button type="button" onClick={() => onJump(c)} className="min-w-0 flex-1 truncate text-left font-mono text-[11px] text-muted-foreground hover:text-foreground" title={`${c.path}:${lines(c)}`}>
                {name(c.path)}:{lines(c)}
              </button>
              <Button variant="ghost" size="icon-xs" onClick={() => onRemove(c.id)} aria-label={`Remove comment on ${name(c.path)}:${lines(c)}`} className="opacity-0 group-hover/note:opacity-100 focus-visible:opacity-100">
                <Trash2Icon />
              </Button>
            </div>
            {c.quote[0] !== undefined && <p className="mt-1 truncate border-l-2 border-hairline-strong pl-2 font-mono text-[11px] text-muted-foreground">{c.quote[0]}</p>}
            <p className="mt-1 line-clamp-3 text-[12px] break-words whitespace-pre-wrap">{c.body}</p>
          </li>
        ))}
      </ul>
      <div className="flex flex-col gap-2 border-t border-hairline p-3">
        <Select
          items={items}
          value={paneId}
          onValueChange={(v) => setPaneId(v)}
          onOpenChange={(open) => {
            if (open) refresh();
          }}
        >
          <SelectTrigger ref={trigger} size="sm" aria-label="Send to" className="w-full">
            <SelectValue placeholder={found.targets.length ? "Choose where to send" : "No agent is running"} />
          </SelectTrigger>
          <SelectContent>
            {agents.length > 0 && (
              <SelectGroup>
                <SelectLabel>Agents</SelectLabel>
                {agents.map((t) => (
                  <SelectItem key={t.paneId} value={t.paneId}>
                    {label(t)}
                  </SelectItem>
                ))}
              </SelectGroup>
            )}
          </SelectContent>
        </Select>
        {error && (
          <p role="alert" className="text-[12px] text-blocked">
            {error}
          </p>
        )}
        <div className="flex gap-1.5">
          <Button size="sm" disabled={!paneId || busy} onClick={() => void send(false)} className="flex-1" title="Pastes the comments into the prompt without pressing Enter">
            {busy ? <Spinner className="size-3.5" /> : <SendIcon aria-hidden />}
            Send to agent
          </Button>
          <Button size="sm" variant="outline" disabled={!paneId || busy} onClick={() => void send(true)} title="Pastes the comments and presses Enter">
            Send and submit
          </Button>
        </div>
      </div>
    </aside>
  );
}

function label(t: ReviewTarget) {
  const where = t.project || (t.cwd ? name(t.cwd) : "");
  return where ? `${t.label} · ${where}` : t.label;
}
