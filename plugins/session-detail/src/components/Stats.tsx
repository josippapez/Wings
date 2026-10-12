import { Stats as Figures, Text } from "wings-plugin-ui";

import { tokens } from "../lib/format";
import type { SessionView } from "../lib/types";
import { status } from "./format";

/** The session's status, model and context, and its tokens this run. */
export function Stats({ s }: { s: SessionView }) {
  return (
    <>
      <Figures
        items={[
          { label: "Status", value: status[s.state] ?? s.state, tone: s.state === "blocked" ? "warn" : undefined },
          { label: "Model", value: s.model || "—" },
          { label: "Context", value: tokens(s.context) },
        ]}
      />
      <Text as="p" tone="muted" size="small" className="tokens">{`Tokens this run: ${s.tokens}`}</Text>
    </>
  );
}
