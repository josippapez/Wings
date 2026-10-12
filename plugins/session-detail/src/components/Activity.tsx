import type { Detail, ItemView } from "../lib/types";
import { Card } from "./Card";
import { clock, took } from "./format";

/** One row of the timeline: a button that opens its card. */
function Row({ item, open, detail, toggle }: { item: ItemView; open: boolean; detail: Detail | undefined; toggle: () => void }) {
  const button = (cls: string, children: React.ReactNode) => (
    <button className={`toggle ${cls}`} id={`item-${item.key}`} aria-expanded={open} onClick={toggle}>
      {children}
    </button>
  );
  return item.kind === "prompt" ? (
    <li className="prompt">
      {button(
        "",
        <>
          <span className="who">You</span>
          <span className={open ? "grow" : "grow clamp"}>{item.summary}</span>
          <time className="muted">{clock(item.at)}</time>
        </>,
      )}
      {open ? <Card item={item} detail={detail} /> : null}
    </li>
  ) : (
    <li className={item.error ? "error" : ""}>
      {button(
        "row",
        <>
          {item.done ? <span className={item.error ? "dot err" : "dot"} aria-hidden /> : <span className="spinner" aria-hidden />}
          {(item.tags ?? []).map((t, i) => (
            <span key={i} className={`tag ${t.kind}`}>
              {t.label}
            </span>
          ))}
          {item.tags ? null : <strong>{item.name}</strong>}
          <span className={item.tags ? "grow ellipsis" : "grow muted ellipsis"}>{item.summary}</span>
          {item.outcome ? <span className={item.outcome.endsWith("failed") ? "tag danger" : "tag ok"}>{item.outcome}</span> : null}
          <span className="muted">{item.error ? "failed" : took(item.ms)}</span>
        </>,
      )}
      {open ? <Card item={item} detail={detail} /> : null}
    </li>
  );
}

/** Recent activity, all of it or only your messages, highlighted commands and failures. */
export function Activity({
  items,
  highlightsOnly,
  setHighlightsOnly,
  isOpen,
  detailOf,
  toggle,
}: {
  items: ItemView[];
  highlightsOnly: boolean;
  setHighlightsOnly: (on: boolean) => void;
  isOpen: (key: string) => boolean;
  detailOf: (key: string) => Detail | undefined;
  toggle: (key: string) => void;
}) {
  const shown = highlightsOnly ? items.filter((i) => i.kind === "prompt" || i.tags || i.error) : items;
  const filter = (on: boolean, label: string) => (
    <button className={`chip${highlightsOnly === on ? " on" : ""}`} aria-pressed={highlightsOnly === on} onClick={() => setHighlightsOnly(on)}>
      {label}
    </button>
  );
  return (
    <>
      <div className="row heading">
        <h2 className="grow">Recent activity</h2>
        <div className="row filters" role="group" aria-label="Show">
          {filter(false, "All")}
          {filter(true, "Highlights")}
        </div>
      </div>
      {shown.length ? (
        <ol className="list timeline">
          {shown.map((item) => (
            <Row key={item.key} item={item} open={isOpen(item.key)} detail={detailOf(item.key)} toggle={() => toggle(item.key)} />
          ))}
        </ol>
      ) : (
        <p className="muted">{highlightsOnly ? "No highlights in this run yet." : "Nothing yet."}</p>
      )}
    </>
  );
}
