import { useEffect, useId, useMemo, useRef, useState } from "react";
import { SearchIcon } from "lucide-react";

import { StatusDot } from "@/components/status-dot";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Kbd } from "@/components/ui/kbd";
import { cn } from "@/lib/utils";
import { groupOf, rank, type PaletteItem } from "@/lib/palette";

/** Typing waits this long before asking the history for sessions, so a word typed at speed asks once. */
const SESSION_DEBOUNCE_MS = 150;

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Commands, projects, tabs, agents and plugin buttons. Read when the palette opens and while it is open. */
  items: PaletteItem[];
  /** Past sessions matching `query` (the most recent ones when it's empty). */
  loadSessions: (query: string) => Promise<PaletteItem[]>;
  /** The terminal that has focus, to give focus back to when the palette closes. */
  focusTerminal: () => void;
};

/** ⌘K: search commands, projects, tabs, agents, recent sessions and plugin buttons in one list. */
export function CommandPalette({ open, onOpenChange, items, loadSessions, focusTerminal }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [sessions, setSessions] = useState<PaletteItem[]>([]);
  const chose = useRef<PaletteItem | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const base = useId();

  useEffect(() => {
    if (!open) return;
    let current = true;
    const timer = setTimeout(() => void loadSessions(query.trim()).then((found) => current && setSessions(found), () => current && setSessions([])), query.trim() ? SESSION_DEBOUNCE_MS : 0);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [open, query, loadSessions]);

  const shown = useMemo(() => rank([...items, ...sessions], query), [items, sessions, query]);
  const at = Math.min(active, Math.max(shown.length - 1, 0));
  const optionId = (i: number) => `${base}-${i}`;

  useEffect(() => {
    list.current?.querySelector(`[id="${optionId(at)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [at, shown]);

  const choose = (item: PaletteItem | undefined) => {
    if (!item || item.disabled) return;
    chose.current = item;
    onOpenChange(false);
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const move = (by: number) => {
      e.preventDefault();
      if (shown.length) setActive((at + by + shown.length) % shown.length);
    };
    if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) move(1);
    else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) move(-1);
    else if (e.key === "Home") setActive(0);
    else if (e.key === "End") setActive(shown.length - 1);
    else if (e.key === "Enter" && !e.nativeEvent.isComposing) {
      e.preventDefault();
      choose(shown[at]);
    }
  };

  // Headings only while nothing is typed: a search is one list in rank order.
  let lastGroup = "";
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) chose.current = null;
        onOpenChange(next);
      }}
      onOpenChangeComplete={(isOpen) => {
        if (isOpen) return;
        setQuery("");
        setActive(0);
        setSessions([]);
        // Esc, or a command that doesn't open something of its own, puts you back in the terminal. A command
        // that changed the pane has already moved focus by now, so this is the pane you're on.
        if (!chose.current?.opensUi) focusTerminal();
        chose.current = null;
      }}
    >
      <DialogContent showCloseButton={false} finalFocus={false} className="top-[16%] translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-xl">
        <DialogTitle className="sr-only">Command palette</DialogTitle>
        <DialogDescription className="sr-only">Search commands, projects, tabs, agents and recent sessions. Arrow keys move, Enter runs, Escape closes.</DialogDescription>
        <div className="flex items-center gap-2 border-b border-hairline px-3">
          <SearchIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
          <input
            autoFocus
            role="combobox"
            aria-expanded
            aria-controls={`${base}-list`}
            aria-activedescendant={shown.length ? optionId(at) : undefined}
            aria-autocomplete="list"
            aria-label="Search commands"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="Search commands, projects, tabs and agents"
            className="h-11 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            spellCheck={false}
            autoComplete="off"
          />
        </div>
        <div ref={list} id={`${base}-list`} role="listbox" aria-label="Results" className="max-h-[min(380px,50vh)] overflow-y-auto p-1.5">
          {shown.length === 0 && <p className="px-3 py-6 text-center text-sm text-muted-foreground">Nothing matches “{query.trim()}”</p>}
          {shown.map((item, i) => {
            const group = groupOf(item);
            const heading = !query.trim() && group !== lastGroup;
            lastGroup = group;
            return (
              <div key={item.id}>
                {heading && <div className="px-2.5 pt-2 pb-1 text-[11px] font-medium text-muted-foreground">{group}</div>}
                <div
                  id={optionId(i)}
                  role="option"
                  aria-selected={i === at}
                  aria-disabled={item.disabled}
                  onPointerMove={() => i !== at && setActive(i)}
                  onClick={() => choose(item)}
                  className={cn("flex h-9 cursor-default items-center gap-2.5 rounded-md px-2.5 text-sm", i === at && "bg-accent text-accent-foreground", item.disabled && "opacity-50")}
                >
                  {item.state && <StatusDot state={item.state} />}
                  <span className="truncate">{item.title}</span>
                  {item.subtitle && <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{item.subtitle}</span>}
                  {!item.subtitle && <span className="flex-1" />}
                  {item.shortcut ? <Kbd>{item.shortcut}</Kbd> : query.trim() ? <span className="shrink-0 text-[11px] text-muted-foreground">{group}</span> : null}
                </div>
              </div>
            );
          })}
        </div>
        <div className="flex items-center gap-3 border-t border-hairline px-3 py-2 text-[11px] text-muted-foreground">
          {[
            ["↑↓", "move"],
            ["↵", "run"],
            ["esc", "close"],
          ].map(([key, what]) => (
            <span key={what} className="flex items-center gap-1">
              <Kbd className="h-4 min-w-4 text-[10px]">{key}</Kbd>
              {what}
            </span>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
