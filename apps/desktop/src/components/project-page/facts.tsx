import { PencilIcon, Trash2Icon } from "lucide-react";
import { useRef, useState, type RefObject } from "react";

import { IconButton } from "@/components/icon-button";
import { MarkdownInline } from "@/components/markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { LIMITS, type Fact } from "@/lib/project-info";

import { cn } from "@/lib/utils";

import { findFact } from "./merge";
import { markClass, type Mark } from "./search";
import { messageOf, type Apply } from "./use-project-info";

/** The add row sits quiet, like an empty property in Notion, until you point at it or type. */
const quiet = "border-dashed border-input bg-transparent placeholder:italic focus-visible:border-solid focus-visible:border-ring dark:bg-transparent";

const byLabel = (by: Fact["by"]) => (by === "agent" ? "Written by an agent" : "Written by you");

/** Key and value inputs: edits a fact, or with `add` is the empty row under the table. */
function FactForm({ initial, add, keyRef, onSave, onCancel }: { initial?: Fact; add?: boolean; keyRef?: RefObject<HTMLInputElement | null>; onSave: (key: string, value: string) => Promise<void>; onCancel?: () => void }) {
  const own = useRef<HTMLInputElement>(null);
  const key = keyRef ?? own;
  const value = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!key.current || !value.current || !key.current.value.trim() || busy) return;
    setBusy(true);
    try {
      await onSave(key.current.value, value.current.value);
      if (add && key.current && value.current) {
        key.current.value = value.current.value = "";
        key.current.focus();
      }
    } catch {
      // The table shows the message; what was typed stays.
    } finally {
      setBusy(false);
    }
  };
  const enter = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") void submit();
    else if (e.key === "Escape") onCancel?.();
  };
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Input ref={key} aria-label="Fact name" placeholder={add ? "Name" : "Dev server"} maxLength={LIMITS.key} defaultValue={initial?.key} autoFocus={!add} onKeyDown={enter} className={cn("w-40 flex-none", add && quiet)} />
      <Input ref={value} aria-label="Fact value" placeholder={add ? "Value" : "pnpm dev, port 5173"} maxLength={LIMITS.value} defaultValue={initial?.value} onKeyDown={enter} className={cn("min-w-48 flex-1", add && quiet)} />
      <Button size="sm" variant={add ? "ghost" : "secondary"} disabled={busy} onClick={() => void submit()}>
        {add ? "Add fact" : "Save"}
      </Button>
      {onCancel && (
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      )}
    </div>
  );
}

/** The facts as a table, each row editable in place, with a row to add one. */
export function FactsTable({ facts, apply, addRef, marks }: { facts: Fact[]; apply: Apply; addRef: RefObject<HTMLInputElement | null>; marks: ReadonlyMap<string, Mark> }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [error, setError] = useState("");
  const run = async (job: Promise<unknown>) => {
    try {
      await job;
      setError("");
    } catch (e) {
      setError(messageOf(e));
      throw e;
    }
  };
  return (
    <section aria-label="Facts" className="flex flex-col gap-1.5">
      <h2 className="flex items-baseline gap-2 px-1 text-[12px] font-medium text-muted-foreground">
        Facts
        {facts.length > 0 && <span className="text-[11px] font-normal tabular-nums opacity-70">{facts.length}</span>}
      </h2>
      {facts.length > 0 && (
        <table className="w-full table-fixed border-collapse text-[13px]">
          <tbody>
            {facts.map((f) =>
              editing === f.key ? (
                <tr key={f.key}>
                  <td colSpan={3} className="py-1">
                    <FactForm initial={f} onCancel={() => setEditing(null)} onSave={(key, value) => run(apply({ kind: "setFact", key, value, oldKey: f.key }).then(() => setEditing(null)))} />
                  </td>
                </tr>
              ) : (
                <tr key={f.key} data-fact-key={f.key} className={cn("group/fact border-t border-hairline first:border-t-0 hover:bg-hover/60 focus-within:bg-hover/60", markClass(marks.get(f.key)))}>
                  <th scope="row" className="w-40 rounded-l-lg px-2 py-1.5 text-left align-top text-[12.5px] font-normal text-muted-foreground break-words">
                    {f.key}
                  </th>
                  <td className="px-2 py-1.5 align-top break-words">
                    <MarkdownInline text={f.value} />
                  </td>
                  <td className="w-[92px] rounded-r-lg px-1 py-1 text-right align-top whitespace-nowrap">
                    <span className={`inline-flex items-center transition-opacity group-focus-within/fact:opacity-100 group-hover/fact:opacity-100 ${removing === f.key ? "opacity-100" : "opacity-0"}`}>
                      <span className="mr-1 text-[11px] text-muted-foreground" title={byLabel(f.by)}>
                        {f.by === "agent" ? "agent" : "you"}
                      </span>
                      <IconButton label={`Edit ${f.key}`} onClick={() => setEditing(f.key)}>
                        <PencilIcon />
                      </IconButton>
                      {removing === f.key ? (
                        <>
                          <Button size="xs" variant="destructive" autoFocus onClick={() => void run(apply({ kind: "removeFact", key: f.key })).then(() => setRemoving(null), () => {})}>
                            Remove
                          </Button>
                          <Button size="xs" variant="ghost" onClick={() => setRemoving(null)}>
                            Keep
                          </Button>
                        </>
                      ) : (
                        <IconButton label={`Remove ${f.key}`} onClick={() => setRemoving(f.key)}>
                          <Trash2Icon />
                        </IconButton>
                      )}
                    </span>
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      )}
      {facts.length < LIMITS.facts ? (
        <FactForm add keyRef={addRef} onSave={(key, value) => {
            // `setFact` replaces a fact with the same key, so adding over one would silently overwrite it.
            const dup = findFact(facts, key);
            if (dup) {
              setEditing(dup.key);
              return run(Promise.reject(new Error(`${dup.key} already exists, edit it instead.`)));
            }
            return run(apply({ kind: "setFact", key, value }));
          }}
        />
      ) : (
        <p className="text-[12px] text-muted-foreground">A project holds at most {LIMITS.facts} facts.</p>
      )}
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      )}
    </section>
  );
}

