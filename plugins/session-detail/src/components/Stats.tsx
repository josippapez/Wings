import { tokens } from "../lib/format";
import type { SessionView } from "../lib/types";
import { status } from "./format";

/** The session's status, model and context, and its tokens this run. */
export function Stats({ s }: { s: SessionView }) {
  return (
    <>
      <dl className="stats">
        <div>
          <dt>Status</dt>
          <dd className={s.state === "blocked" ? "warn" : ""}>{status[s.state] ?? s.state}</dd>
        </div>
        <div>
          <dt>Model</dt>
          <dd>{s.model || "—"}</dd>
        </div>
        <div>
          <dt>Context</dt>
          <dd>{tokens(s.context)}</dd>
        </div>
      </dl>
      <p className="muted tokens">{`Tokens this run: ${s.tokens}`}</p>
    </>
  );
}
