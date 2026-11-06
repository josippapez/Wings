import { useQuery } from "wings-plugin-sdk/react";
import { Card as Panel, Text } from "wings-plugin-ui";

import { ch } from "../lib/messages";
import type { ItemView } from "../lib/types";
import { took } from "./format";

/** What a row opens to: the prompt in full, or a tool call's inputs, timing and the start of its result. */
export function Card({ sessionId, item }: { sessionId: string; item: ItemView }) {
  // Main sends it again when the session changes, so a call still running when the row opened gets its result.
  const { data: d, error } = useQuery(ch, "detail", { sessionId, key: item.key });
  if (!d)
    return (
      <Panel compact className="detail">
        <Text tone={error ? "danger" : "muted"}>{error ? error.message : "Loading…"}</Text>
      </Panel>
    );
  const out = (part: string, text: string, warn = false) => (
    <Text as="div" key={part} id={`pre-${item.key}-${part}`} className="out" mono size="tiny" tone={warn ? "warn" : undefined}>
      {text}
    </Text>
  );
  if (d.kind === "prompt")
    return (
      <Panel compact className="detail">
        {out("text", d.text!)}
      </Panel>
    );
  const at = new Date(d.at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  return (
    <Panel compact className="detail">
      <Text as="p" tone="muted">
        {`Started ${at}`}
        {d.done ? (d.ms == null ? "" : ` · ${d.error ? "failed after" : "took"} ${took(d.ms)}`) : " · still running"}
      </Text>
      {d.input!.flatMap(([key, value]) => [
        <Text as="div" key={`label-${key}`} tone="muted" size="tiny">
          {key}
        </Text>,
        out(key, value),
      ])}
      {d.result != null ? (
        <Text as="div" tone="muted" size="tiny">
          {d.error ? "Error" : "Result"}
        </Text>
      ) : null}
      {d.result != null ? out("result", d.result || "(no output)", d.error) : null}
    </Panel>
  );
}
