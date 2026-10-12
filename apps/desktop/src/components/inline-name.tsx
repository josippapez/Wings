import { useEffect, useRef } from "react";

import { cn } from "@/lib/utils";

/**
 * A name being typed in place. Enter or leaving the field saves, Escape cancels. `onDone` gets the new text (empty
 * resets to the automatic name), or null when it was cancelled or left as it was.
 */
export function InlineName({ initial, label, className, onDone }: { initial: string; label: string; className?: string; onDone: (name: string | null) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);
  const finish = (name: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(name === null || name.trim() === initial ? null : name.trim());
  };
  return (
    <input
      ref={input}
      defaultValue={initial}
      aria-label={label}
      placeholder="Automatic name"
      maxLength={60}
      spellCheck={false}
      onKeyDown={(e) => {
        // Typing here is not a shortcut for the tab strip, the panes or the app.
        e.stopPropagation();
        if (e.key === "Enter") finish(e.currentTarget.value);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.currentTarget.value)}
      onPointerDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      className={cn("min-w-0 bg-transparent outline-none placeholder:text-muted-foreground", className)}
    />
  );
}
