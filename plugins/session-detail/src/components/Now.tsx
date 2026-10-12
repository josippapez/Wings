import type { SessionView } from "../lib/types";

/** The tool calls still running. */
export function Now({ running }: { running: SessionView["running"] }) {
  return (
    <>
      <h2>Now</h2>
      <ul className="list">
        {running.map((t, i) => (
          <li key={i} className="row">
            <span className="spinner" aria-hidden />
            <strong>{t.name}</strong>
            <span className="grow muted ellipsis">{t.summary}</span>
          </li>
        ))}
      </ul>
    </>
  );
}
