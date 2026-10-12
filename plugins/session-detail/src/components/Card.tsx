import { Text } from "wings-plugin-ui";

import type { Detail, ItemView } from "../lib/types";
import { took } from "./format";

/** What a row opens to: the prompt in full, or a tool call's inputs, timing and the start of its result. */
export function Card({ item, detail: d }: { item: ItemView; detail: Detail | undefined }) {
  if (!d)
    return (
      <div className="detail">
        <Text tone="muted">Loading…</Text>
      </div>
    );
  const pre = (part: string, text: string, cls = "") => (
    <pre key={part} id={`pre-${item.key}-${part}`} className={cls}>
      {text}
    </pre>
  );
  if (d.kind === "prompt") return <div className="detail">{pre("text", d.text!)}</div>;
  const at = new Date(d.at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  return (
    <div className="detail">
      <Text as="p" tone="muted">
        {`Started ${at}`}
        {d.done ? (d.ms == null ? "" : ` · ${d.error ? "failed after" : "took"} ${took(d.ms)}`) : " · still running"}
      </Text>
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
