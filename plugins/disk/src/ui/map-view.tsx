// The map of a finished scan: the circle you click into, the list of what is in the folder beside it, and the Collector under
// both. A view with no data of its own: the tab page feeds it from main, a fixture from made-up data. It keeps what only the
// screen knows (what the pointer is on, what is picked, which rows are open), and asks its callbacks for everything that
// touches the disk.

import { useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { Page, Split, useAction } from "wings-plugin-ui";
import type { Collected, Collector, Entry, TrashResult } from "../lib/messages";
import type { Target } from "../lib/sunburst";
import { find, parentPath } from "../lib/tree";
import { CollectorBar } from "./collector-bar";
import { EntryList } from "./entry-list";
import { HiddenPanel, type HiddenConfirm } from "./hidden-panel";
import { collectedOf, entryKey, HIDDEN_KEY, rowsOf, smallerKey, type Extras, type Row } from "./list-model";
import { MapToolbar } from "./map-toolbar";
import { crumbsOf, trashMessages, type DoneScan } from "./model";
import { Dismissible, type Message } from "./parts";
import { Sunburst } from "./sunburst";
import { swatchesOf } from "./swatches";

export interface MapViewProps {
  scan: DoneScan;
  /** What the scan's root is called: the disk's name, or the folder's. */
  name: string;
  /** The folder in the middle of the map: a path in the scan's tree. */
  focus: string;
  canBack: boolean;
  canForward: boolean;
  /** The list's width in px. */
  split: number;
  collector: Collector;
  onSplit: (size: number) => void;
  onDisks: () => void;
  onBack: () => void;
  onForward: () => void;
  /** Goes into a folder, which `focus` follows. */
  onFocus: (path: string) => void;
  onRescan: () => void;
  onReveal: (path: string) => Promise<unknown>;
  onPreview: (path: string) => Promise<unknown>;
  /** Rejects with why when a path may not be collected. */
  onCollect: (items: Collected[]) => Promise<unknown>;
  onUncollect: (paths: string[]) => Promise<unknown>;
  onTrash: () => Promise<TrashResult>;
  /** Every entry directly in a folder. */
  onListAll: (folder: string) => Promise<Entry>;
  onPurge: (mount: string) => Promise<unknown>;
  onDeleteSnapshot: (date: string) => Promise<unknown>;
  /** For a fixture: what the pointer starts on, and how long Move to Trash waits. */
  initialPoint?: Target;
  countdown?: number;
}

/** The folder the pointer is on, when it is on a folder's wedge or on a folder's smaller items. */
function folderUnder(tree: Entry, point: Target | null): string | null {
  if (!point || point.kind === "hidden") return null;
  if (point.kind === "smaller") return point.folder;
  const entry = find(tree, point.path)?.at(-1);
  return entry && entry.kind === "dir" && !entry.restricted ? point.path : null;
}

export function MapView(props: MapViewProps) {
  const { scan, name, focus, collector, countdown } = props;
  const tree = scan.tree;
  const scanKey = `${scan.root}@${scan.at}`;

  const [point, setPoint] = useState<Target | null>(props.initialPoint ?? null);
  const [selected, setSelected] = useState<string | null>(null);
  const [hiddenOpen, setHiddenOpen] = useState(false);
  const [confirm, setConfirm] = useState<HiddenConfirm | null>(null);
  const [smaller, setSmaller] = useState<ReadonlySet<string>>(new Set());
  const [extras, setExtras] = useState<Record<string, Extras>>({});
  const [messages, setMessages] = useState<Message[]>([]);
  const ids = useRef(0);
  const action = useAction();
  // The list never starts out taking more than nine twentieths of a narrow tab: the map is what it is for.
  const [listWidth] = useState(() => Math.min(props.split, Math.max(200, Math.round(window.innerWidth * 0.45))));

  // Going into another folder starts with nothing picked.
  const [at, setAt] = useState(focus);
  if (at !== focus) {
    setAt(focus);
    setPoint(null);
    setSelected(null);
    setHiddenOpen(false);
    setConfirm(null);
  }

  const hidden = focus === tree.name && scan.disk && scan.hidden && scan.hidden.size > 0 ? { info: scan.hidden, mount: scan.disk } : null;
  const hiddenSize = hidden?.info.size ?? null;
  const showHidden = hiddenOpen && hidden !== null;
  const listPath = folderUnder(tree, point) ?? focus;
  const listEntry = find(tree, listPath)?.at(-1) ?? tree;
  const extrasKey = `${scanKey}\n${listPath}`;
  const rows = useMemo(
    () => rowsOf(listPath, listEntry, { smallerOpen: smaller.has(listPath), extras: extras[extrasKey], hidden: listPath === tree.name ? hiddenSize : null }),
    [listPath, listEntry, smaller, extras, extrasKey, tree, hiddenSize],
  );
  const swatch = useMemo(() => swatchesOf(tree, focus, hiddenSize), [tree, focus, hiddenSize]);
  const collected = useMemo(() => new Set(collector.items.map((i) => i.path)), [collector.items]);
  const crumbs = useMemo(() => crumbsOf(tree, focus, name), [tree, focus, name]);
  const row = rows.find((r) => r.key === selected);
  const pointed = !point ? null : point.kind === "entry" ? entryKey(point.path) : point.kind === "smaller" ? smallerKey(point.folder) : HIDDEN_KEY;

  const say = (list: Omit<Message, "id">[]) => setMessages(list.map((m) => ({ ...m, id: ++ids.current })));
  const run = (fn: () => Promise<unknown>) => void action.run(fn);

  const loadExtras = (folder: string) => {
    const key = `${scanKey}\n${folder}`;
    if (extras[key]?.state === "ready" || extras[key]?.state === "loading") return;
    setExtras((e) => ({ ...e, [key]: { state: "loading" } }));
    props.onListAll(folder).then(
      (all) => setExtras((e) => ({ ...e, [key]: { state: "ready", entries: all.children ?? [] } })),
      (error) => setExtras((e) => ({ ...e, [key]: { state: "error", message: errorMessage(error) } })),
    );
  };
  const toggleSmaller = (folder: string) => {
    const wasOpen = smaller.has(folder);
    setSmaller((s) => {
      const next = new Set(s);
      if (wasOpen) next.delete(folder);
      else next.add(folder);
      return next;
    });
    if (!wasOpen) loadExtras(folder);
  };

  const collect = (items: Collected[]) => {
    say([]);
    props.onCollect(items).catch((error) => say([{ tone: "warn", text: errorMessage(error) }]));
  };
  const toggleCollect = (r: Row) => {
    const item = collectedOf(r);
    if (!item) return;
    if (collected.has(item.path)) run(() => props.onUncollect([item.path]));
    else collect([item]);
  };
  const trash = async () => {
    say([]);
    try {
      say(trashMessages(await props.onTrash()));
    } catch (error) {
      say([{ tone: "danger", text: errorMessage(error) }]);
    }
  };

  const up = () => {
    const parent = focus === tree.name ? null : parentPath(focus);
    if (parent) props.onFocus(parent);
  };
  /** A click on a row, or Enter on it: a folder is gone into, and the smaller items and the hidden space are opened. */
  const open = (r: Row) => {
    if (r.kind === "entry" && r.openable) props.onFocus(r.path);
    else if (r.kind === "smaller") toggleSmaller(r.path);
    else if (r.kind === "hidden") setHiddenOpen(true);
  };
  const wedge = (target: Target, { meta }: { meta: boolean }) => {
    if (target.kind === "hidden") {
      if (!meta) setHiddenOpen(true);
      return;
    }
    const path = target.kind === "entry" ? target.path : target.folder;
    const entry = find(tree, path)?.at(-1);
    if (meta) {
      run(() => props.onReveal(path));
    } else if (target.kind === "smaller") {
      // Its row is in the folder's list, so go into the folder and open the row.
      if (path !== focus) props.onFocus(path);
      if (!smaller.has(path)) toggleSmaller(path);
      setSelected(smallerKey(path));
    } else if (entry?.kind === "dir" && !entry.restricted) {
      props.onFocus(path);
    } else {
      setSelected(entryKey(path));
    }
  };

  // The keys act on what the latest render shows. One listener lives as long as the view.
  const keys = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.isComposing) return;
    const target = e.target instanceof Element ? e.target : null;
    // Typing, a menu, a dialog and the split's handle keep their keys.
    if (target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false]), [role=menu], [role=listbox], [role=combobox], [role=alertdialog], [role=separator]")) return;
    const plain = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;
    const cmd = e.metaKey && !e.ctrlKey && !e.altKey;
    // Enter and Space belong to a button that has focus, unless it is a row, which they act on.
    const control = target?.closest("button, a, [role=menuitem]");
    const own = !!control && !control.hasAttribute("data-row");
    const step = (by: 1 | -1) => {
      if (rows.length === 0) return;
      const i = rows.findIndex((r) => r.key === selected);
      setSelected(rows[i < 0 ? (by === 1 ? 0 : rows.length - 1) : Math.min(rows.length - 1, Math.max(0, i + by))].key);
    };
    const todo = ((): (() => void) | null => {
      switch (e.key) {
        case "ArrowDown":
        case "ArrowUp":
          if (cmd && e.key === "ArrowUp") return up;
          return plain && !showHidden && !target?.closest("[aria-haspopup]") ? () => step(e.key === "ArrowDown" ? 1 : -1) : null;
        case "Backspace":
          if (plain) return up;
          return cmd && row ? () => toggleCollect(row) : null;
        case "Enter":
          return plain && !own && row ? () => open(row) : null;
        case " ":
          return plain && !own && row && collectedOf(row) ? () => run(() => props.onPreview(row.path)) : null;
        case "Escape":
          if (!plain) return null;
          if (showHidden) {
            return () => {
              setHiddenOpen(false);
              setSelected(null);
            };
          }
          return selected ? () => setSelected(null) : null;
        default:
          return null;
      }
    })();
    if (todo) {
      e.preventDefault();
      todo();
    }
  };
  const latest = useRef(keys);
  latest.current = keys;
  useEffect(() => {
    const down = (e: KeyboardEvent) => latest.current(e);
    // A row with focus would click on Space's key up.
    const released = (e: KeyboardEvent) => {
      if (e.key === " " && e.target instanceof Element && e.target.closest("[data-row]")) e.preventDefault();
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", released);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", released);
    };
  }, []);

  return (
    <Page
      flush
      className="dk-map-page"
      toolbar={
        <MapToolbar
          crumbs={crumbs}
          canBack={props.canBack}
          canForward={props.canForward}
          onDisks={props.onDisks}
          onBack={props.onBack}
          onForward={props.onForward}
          onFocus={props.onFocus}
          onRescan={props.onRescan}
          onReveal={() => run(() => props.onReveal(focus))}
        />
      }
    >
      {action.error ? <Dismissible tone="danger" onDismiss={action.clear}>{action.error}</Dismissible> : null}
      <Split
        className="dk-split"
        defaultSize={listWidth}
        min={200}
        label="Resize the list"
        onResize={props.onSplit}
        start={
          <div className="dk-list-pane">
            {showHidden && hidden ? (
              <HiddenPanel hidden={hidden.info} mount={hidden.mount} confirm={confirm} onConfirm={setConfirm} onBack={() => setHiddenOpen(false)} onPurge={props.onPurge} onDeleteSnapshot={props.onDeleteSnapshot} />
            ) : (
              <EntryList
                key={listPath}
                name={listPath === tree.name ? name : listEntry.name}
                size={listEntry.size}
                items={listEntry.items}
                refining={scan.refining.includes(listPath)}
                rows={rows}
                swatch={(r) => swatch(r.path)}
                selected={selected}
                pointed={pointed}
                collected={collected}
                smallerOpen={smaller.has(listPath)}
                extras={extras[extrasKey]}
                onRow={(r) => {
                  setSelected(r.key);
                  open(r);
                }}
                onFocusRow={(r) => setSelected(r.key)}
                onToggleCollect={toggleCollect}
                onReveal={(r) => run(() => props.onReveal(r.path))}
                onPreview={(r) => run(() => props.onPreview(r.path))}
              />
            )}
          </div>
        }
        end={
          <div className="dk-map">
            <Sunburst tree={tree} focus={focus} rootLabel={name} hidden={hiddenSize} collected={collected} selected={row?.kind === "entry" ? row.path : null} onPoint={setPoint} onClick={wedge} onUp={up} />
          </div>
        }
      />
      <CollectorBar
        items={collector.items}
        total={collector.total}
        countdown={countdown}
        messages={messages}
        onDismiss={(id) => setMessages((m) => m.filter((x) => x.id !== id))}
        onDrop={collect}
        onRemove={(path) => run(() => props.onUncollect([path]))}
        onReveal={(path) => run(() => props.onReveal(path))}
        onTrash={trash}
      />
    </Page>
  );
}
