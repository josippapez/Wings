// The list beside the map: the folder the pointer is on, or else the one in the middle, with what is in it. A folder can hold
// thousands of entries, so only the rows in view are drawn: every row is `ROW` tall and sits where its place in the list says.

import { useEffect, useLayoutEffect, useRef, useState, type DragEvent } from "react";
import { Dot, Icon, List, ListButton, ListItem, Menu, Row, Spinner, Text, Title } from "wings-plugin-ui";
import { bytes } from "../lib/format";
import { collectedOf, type Extras, type Row as ListRow } from "./list-model";
import { count } from "./model";
import { DRAG_TYPE } from "./sunburst";

export interface EntryListProps {
  /** The folder's name, size and the files and folders in it. */
  name: string;
  size: number;
  items: number;
  /** Main is scanning the folder in more detail. */
  refining: boolean;
  rows: ListRow[];
  /** The colour of a row's wedge, or null when the map draws none. */
  swatch: (row: ListRow) => string | null;
  selected: string | null;
  /** The row the pointer is on in the map. */
  pointed: string | null;
  collected: ReadonlySet<string>;
  /** Whether the smaller items are open, and what `listAll` is doing for them. */
  smallerOpen: boolean;
  extras: Extras | undefined;
  onRow: (row: ListRow) => void;
  /** A row took the keyboard focus. */
  onFocusRow: (row: ListRow) => void;
  onToggleCollect: (row: ListRow) => void;
  onReveal: (row: ListRow) => void;
  onPreview: (row: ListRow) => void;
}

/** How tall a row is, in px. */
const ROW = 28;
/** Rows drawn past either edge of the view, so a scroll shows rows at once. */
const OVERSCAN = 8;

const NO_ACCESS = "Wings can't open this folder. Allow Full Disk Access for Wings in System Settings, under Privacy & Security, to see it.";

interface RowProps extends Pick<EntryListProps, "onRow" | "onFocusRow" | "onToggleCollect" | "onReveal" | "onPreview"> {
  row: ListRow;
  /** Its place in the list of `total` rows. */
  index: number;
  total: number;
  color: string | null;
  selected: boolean;
  pointed: boolean;
  collected: boolean;
  /** The smaller items' row: open, and waiting for what is in it. */
  open: boolean;
  loading: boolean;
}

function EntryRow({ row, index, total, color, selected, pointed, collected, open, loading, onRow, onFocusRow, onToggleCollect, onReveal, onPreview }: RowProps) {
  const item = collectedOf(row);
  const files = row.kind === "entry" || row.kind === "extra";
  const menu = [
    ...(item ? [{ id: "collect", label: collected ? "Remove from Collector" : "Add to Collector", onSelect: () => onToggleCollect(row) }] : []),
    ...(files ? [{ id: "reveal", label: "Reveal in Finder", onSelect: () => onReveal(row) }] : []),
    ...(files && !row.restricted ? [{ id: "preview", label: "Quick Look", onSelect: () => onPreview(row) }] : []),
  ];
  const label = row.kind === "smaller" ? count(row.items, "smaller item") : row.name;
  const dragStart = (e: DragEvent) => {
    e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(item));
    e.dataTransfer.effectAllowed = "copy";
  };
  return (
    <ListItem
      className="dk-row"
      data-key={row.key}
      data-kind={row.kind}
      data-selected={selected ? "" : undefined}
      data-pointed={pointed ? "" : undefined}
      data-collected={collected ? "" : undefined}
      draggable={item ? true : undefined}
      onDragStart={item ? dragStart : undefined}
      style={{ top: index * ROW, height: ROW }}
      aria-posinset={index + 1}
      aria-setsize={total}
    >
      <ListButton
        className="dk-row-main"
        data-row=""
        aria-current={selected ? "true" : undefined}
        aria-expanded={row.kind === "smaller" ? open : undefined}
        title={row.restricted ? NO_ACCESS : collected ? `${row.name} is in the Collector` : undefined}
        onClick={() => onRow(row)}
        onFocus={() => onFocusRow(row)}
      >
        {row.restricted ? <Dot tone="info" /> : <Dot color={color ?? undefined} tone={color ? undefined : "muted"} />}
        <Text grow truncate>{label}</Text>
        {collected ? <Icon name="check" label="In the Collector" className="dk-collected" /> : null}
        {row.restricted ? <Text tone="info" size="small">No access</Text> : <Text numeric tone="muted" className="dk-size">{bytes(row.size)}</Text>}
        {loading ? <Spinner className="dk-chevron" /> : row.openable || row.kind === "hidden" || row.kind === "smaller" ? <Icon name="chevron-down" className="dk-chevron" data-open={open ? "" : undefined} /> : <span className="dk-chevron-space" />}
      </ListButton>
      {menu.length ? (
        <Menu className="dk-row-menu" variant="ghost" size="icon-xs" align="end" label="⋯" aria-label={`More for ${row.name}`} items={menu} />
      ) : (
        <span className="dk-row-menu" />
      )}
    </ListItem>
  );
}

export function EntryList({ name, size, items, refining, rows, swatch, selected, pointed, collected, smallerOpen, extras, onRow, onFocusRow, onToggleCollect, onReveal, onPreview }: EntryListProps) {
  const total = rows.length;
  const body = useRef<HTMLDivElement>(null);
  const place = useRef<HTMLDivElement>(null);
  const hadFocus = useRef(false);
  const [range, setRange] = useState<[number, number]>([0, 0]);

  // The rows in view, from where the list is scrolled to. It follows the scroll, the size of the pane and the number of rows.
  const read = () => {
    const el = body.current;
    const at = place.current;
    if (!el || !at) return;
    const top = el.scrollTop - at.offsetTop;
    const first = Math.max(0, Math.floor(top / ROW) - OVERSCAN);
    const last = Math.min(total, Math.ceil((top + el.clientHeight) / ROW) + OVERSCAN);
    setRange((r) => (r[0] === first && r[1] === last ? r : [first, last]));
  };
  const latest = useRef(read);
  latest.current = read;
  useLayoutEffect(() => {
    const el = body.current;
    if (!el) return;
    const again = () => latest.current();
    const observer = new ResizeObserver(again);
    observer.observe(el);
    el.addEventListener("scroll", again, { passive: true });
    return () => {
      observer.disconnect();
      el.removeEventListener("scroll", again);
    };
  }, []);
  useLayoutEffect(read, [total]);

  // The row that is picked comes into view, and keeps the keyboard when it was in the list: the arrow keys move the pick.
  useLayoutEffect(() => {
    const el = body.current;
    const at = place.current;
    const i = selected === null ? -1 : rows.findIndex((r) => r.key === selected);
    if (!el || !at || i < 0) return;
    const top = at.offsetTop + i * ROW;
    if (top < el.scrollTop) el.scrollTop = i === 0 ? 0 : top;
    else if (top + ROW > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW - el.clientHeight;
    read();
    // Only a new pick moves the view: the rows changing under it doesn't.
  }, [selected]);
  useEffect(() => {
    if (hadFocus.current && selected !== null) body.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(selected)}"] [data-row]`)?.focus({ preventScroll: true });
  }, [selected, range]);

  return (
    <div className="dk-list" data-folder={name}>
      <header className="dk-list-head">
        <Title as="h1" truncate title={name}>{name}</Title>
        <Row gap="m" className="dk-list-sub">
          <Text tone="muted" size="small">{bytes(size)}{items > 0 ? `, ${count(items, "item")}` : ""}</Text>
          {refining ? (
            <Row gap="s" role="status">
              <Spinner />
              <Text tone="muted" size="small">Adding detail…</Text>
            </Row>
          ) : null}
        </Row>
        {smallerOpen && extras?.state === "error" ? <Text tone="danger" size="small">Couldn't read the folder: {extras.message}</Text> : null}
      </header>
      <div
        className="dk-list-body"
        ref={body}
        onFocus={() => {
          hadFocus.current = true;
        }}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget)) hadFocus.current = false;
        }}
      >
        {total === 0 ? (
          <Text tone="muted" size="small" className="dk-hint">Nothing in this folder.</Text>
        ) : (
          <div className="dk-rows" ref={place} style={{ height: total * ROW }}>
            <List compact aria-label={`In ${name}`}>
              {rows.slice(range[0], range[1]).map((row, i) => (
                <EntryRow
                  key={row.key}
                  row={row}
                  index={range[0] + i}
                  total={total}
                  color={row.entryKind === "dir" ? swatch(row) : null}
                  selected={row.key === selected}
                  pointed={row.key === pointed}
                  collected={collected.has(row.path)}
                  open={row.kind === "smaller" ? smallerOpen : false}
                  loading={row.kind === "smaller" && smallerOpen && extras?.state === "loading"}
                  onRow={onRow}
                  onFocusRow={onFocusRow}
                  onToggleCollect={onToggleCollect}
                  onReveal={onReveal}
                  onPreview={onPreview}
                />
              ))}
            </List>
          </div>
        )}
        {rows.some((r) => r.restricted) ? <Text tone="muted" size="small" className="dk-hint">Folders marked No access need Full Disk Access for Wings. Allow it in System Settings, under Privacy & Security, to see them.</Text> : null}
      </div>
    </div>
  );
}
