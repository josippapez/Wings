import { PencilIcon, Trash2Icon } from "lucide-react";
import { useRef, useState, type RefObject } from "react";

import { IconButton } from "@/components/icon-button";
import { MarkdownInline } from "@/components/markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { LIMITS, type Fact } from "@/lib/project-info";

import { findFact } from "./merge";
import { messageOf, type Apply } from "./use-project-info";

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
      <Input ref={key} aria-label="Fact name" placeholder="Dev server" maxLength={LIMITS.key} defaultValue={initial?.key} autoFocus={!add} onKeyDown={enter} className="w-44 flex-none" />
      <Input ref={value} aria-label="Fact value" placeholder="pnpm dev, port 5173" maxLength={LIMITS.value} defaultValue={initial?.value} onKeyDown={enter} className="min-w-48 flex-1" />
      <Button size="sm" variant="secondary" disabled={busy} onClick={() => void submit()}>
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
export function FactsTable({ facts, apply, addRef }: { facts: Fact[]; apply: Apply; addRef: RefObject<HTMLInputElement | null> }) {
  const [editing, setEditing] = useState<string | null>(null);
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
    <section aria-label="Facts" className="flex flex-col gap-2">
      <h2 className="text-[13px] font-semibold text-muted-foreground">Facts</h2>
      {facts.length > 0 && (
        <div className="overflow-x-auto rounded-xl border border-hairline">
          <table className="w-full border-collapse text-[13px]">
            <tbody>
              {facts.map((f) =>
                editing === f.key ? (
                  <tr key={f.key} className="border-b border-hairline last:border-b-0">
                    <td colSpan={3} className="p-2">
                      <FactForm initial={f} onCancel={() => setEditing(null)} onSave={(key, value) => run(apply({ kind: "setFact", key, value, oldKey: f.key }).then(() => setEditing(null)))} />
                    </td>
                  </tr>
                ) : (
                  <tr key={f.key} className="group/fact border-b border-hairline last:border-b-0">
                    <th scope="row" className="w-44 px-3 py-1.5 text-left align-top font-medium">
                      {f.key}
                    </th>
                    <td className="px-3 py-1.5 align-top break-words">
                      <MarkdownInline text={f.value} />
                    </td>
                    <td className="w-px px-2 py-1 align-top whitespace-nowrap">
                      <span className="mr-1 text-[11px] text-muted-foreground" title={byLabel(f.by)}>
                        {f.by === "agent" ? "agent" : "you"}
                      </span>
                      <span className="inline-flex opacity-0 transition-opacity group-focus-within/fact:opacity-100 group-hover/fact:opacity-100">
                        <IconButton label={`Edit ${f.key}`} onClick={() => setEditing(f.key)}>
                          <PencilIcon />
                        </IconButton>
                        <IconButton label={`Remove ${f.key}`} onClick={() => void run(apply({ kind: "removeFact", key: f.key })).catch(() => {})}>
                          <Trash2Icon />
                        </IconButton>
                      </span>
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
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

