import type { Detail, ItemView } from "../lib/types";
import { took } from "./format";

/** What a row opens to: the prompt in full, or a tool call's inputs, timing and the start of its result. */
export function Card({ item, detail: d }: { item: ItemView; detail: Detail | undefined }) {
  if (!d) return <div className="card muted">Loading…</div>;
  const pre = (part: string, text: string, cls = "") => (
    <pre key={part} id={`pre-${item.key}-${part}`} className={cls}>
      {text}
    </pre>
  );
  if (d.kind === "prompt") return <div className="card">{pre("text", d.text!)}</div>;
  const at = new Date(d.at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  return (
    <div className="card">
      <p className="muted">
        {`Started ${at}`}
        {d.done ? (d.ms == null ? "" : ` · ${d.error ? "failed after" : "took"} ${took(d.ms)}`) : " · still running"}
      </p>
      {d.input!.flatMap(([key, value]) => [
        <div key={`label-${key}`} className="label">
          {key}
        </div>,
        pre(key, value),
      ])}
      {d.result != null ? <div className="label">{d.error ? "Error" : "Result"}</div> : null}
      {d.result != null ? pre("result", d.result || "(no output)", d.error ? "warn" : "") : null}
    </div>
  );
}
