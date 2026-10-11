import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { ArrowDownIcon, ArrowUpIcon, CaseSensitiveIcon, RegexIcon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { Input } from "@/components/ui/input";
import { Toggle } from "@/components/ui/toggle";
import type { TerminalSession } from "@/lib/terminal";

/** Find in a pane's scrollback. Opens on the pane's ⌘F, Escape closes it and gives the terminal its focus back. */
export function FindBar({ session }: { session: TerminalSession }) {
  const open = useSyncExternalStore(session.subscribeFind, session.getFindOpen);
  const tick = useSyncExternalStore(session.subscribeFind, session.getFindTick);
  const results = useSyncExternalStore(session.subscribeFind, session.getFindResults);
  const input = useRef<HTMLInputElement>(null);
  const [term, setTerm] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const options = { caseSensitive, regex };

  useEffect(() => {
    if (!open) return;
    input.current?.focus();
    input.current?.select();
  }, [open, tick]);

  if (!open) return null;

  const search = (next: string, opts: typeof options, direction: "next" | "previous", incremental = false) =>
    session.find(next, opts, direction, incremental);
  const label = !term ? "" : results.count === 0 ? "No results" : `${results.index + 1} of ${results.count >= 1000 ? "1000+" : results.count}`;

  return (
    <div
      role="search"
      className="absolute top-10 right-3 z-10 flex items-center gap-1 rounded-xl border border-hairline-strong bg-popover/95 p-1 shadow-[0_8px_24px_rgb(0_0_0/0.35)] backdrop-blur-xl"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          session.closeFind();
        }
      }}
    >
      <Input
        ref={input}
        value={term}
        placeholder="Find"
        aria-label="Find in terminal"
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        className="h-7 w-44 border-0 bg-transparent text-[13px] focus-visible:ring-0 md:text-[13px]"
        onChange={(e) => {
          setTerm(e.target.value);
          search(e.target.value, options, "next", true);
        }}
        onKeyDown={(e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          search(term, options, e.shiftKey ? "previous" : "next");
        }}
      />
      <span aria-live="polite" className="min-w-14 text-right text-[12px] text-muted-foreground tabular-nums">
        {label}
      </span>
      <Toggle
        size="sm"
        aria-label="Match case"
        pressed={caseSensitive}
        onPressedChange={(on) => {
          setCaseSensitive(on);
          search(term, { caseSensitive: on, regex }, "next");
        }}
      >
        <CaseSensitiveIcon />
      </Toggle>
      <Toggle
        size="sm"
        aria-label="Regular expression"
        pressed={regex}
        onPressedChange={(on) => {
          setRegex(on);
          search(term, { caseSensitive, regex: on }, "next");
        }}
      >
        <RegexIcon />
      </Toggle>
      <IconButton label="Previous match" shortcut="⇧↩" onClick={() => search(term, options, "previous")}>
        <ArrowUpIcon />
      </IconButton>
      <IconButton label="Next match" shortcut="↩" onClick={() => search(term, options, "next")}>
        <ArrowDownIcon />
      </IconButton>
      <IconButton label="Close find" shortcut="Esc" onClick={() => session.closeFind()}>
        <XIcon />
      </IconButton>
    </div>
  );
}
