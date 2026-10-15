import { useEffect, useRef, useState } from "react";
import { TextInput } from "wings-plugin-ui";

/** A one-line input for editing in place: Enter or leaving it saves, Escape cancels. Empty saves count as a cancel when `required`. */
export function InlineInput({ label, value, max, placeholder, required, className, onSave, onDone }: { label: string; value: string; max: number; placeholder?: string; required?: boolean; className?: string; onSave: (value: string) => void; onDone: () => void }) {
  const [draft, setDraft] = useState(value);
  const done = useRef(false);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const finish = (save: boolean) => {
    if (done.current) return;
    done.current = true;
    const next = draft.trim();
    if (save && next !== value && !(required && !next)) onSave(next);
    onDone();
  };
  return (
    <TextInput
      ref={ref}
      label={label}
      className={className}
      value={draft}
      maxLength={max}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={() => finish(true)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(true);
        if (e.key === "Escape") finish(false);
      }}
    />
  );
}
