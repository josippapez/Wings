// wings-plugin-ui/native: the components of ui.js that the native Wings app draws, rendered by React into a JSON tree
// instead of into a DOM. The page runs in a JS engine with no document, and the app turns each tree into its own
// controls. Handlers never leave the page: the tree names them by id, and the app sends the id back when one fires.
import { createContext, createElement as h, Fragment, useCallback, useContext, useEffect, useRef, useState } from "react";
import Reconciler from "react-reconciler";
import { ConcurrentRoot, DefaultEventPriority } from "react-reconciler/constants.js";
import { errorMessage } from "wings-plugin-sdk/async";
import { ago } from "wings-plugin-sdk/format";
import { emojify, parseBlocks, parseInline } from "./markdown.js";
import { languageOf } from "./highlight.js";

const root = { id: 0, type: "#root", props: {}, children: [] };
let nextId = 1;
// The instances of the tree the app is showing, by id. Rebuilt on every commit from that tree alone, so an instance
// React created for a render it later threw away (or hasn't committed yet) can never be reached by an event.
let shown = new Map();
let lastJson = "";
let priority = 0;

/** Function props become `<instance id>:<prop>`: the id doesn't change while the instance lives, so the app can keep it. */
function setProps(instance, props) {
  const plain = {};
  const fns = {};
  for (const [name, value] of Object.entries(props)) {
    if (name === "children" || value === undefined) continue;
    if (typeof value === "function") {
      fns[name] = value;
      plain[name] = `${instance.id}:${name}`;
    } else {
      plain[name] = value;
    }
  }
  instance.props = plain;
  // Not enumerable, so it stays out of the JSON, and replaced on every update so an event runs the latest function.
  Object.defineProperty(instance, "fns", { value: fns, writable: true, configurable: true, enumerable: false });
}

function collect(node, into) {
  into.set(node.id, node);
  if (node.children) for (const child of node.children) collect(child, into);
}

function commit() {
  const live = new Map();
  collect(root, live);
  shown = live;
  const json = JSON.stringify(root);
  if (json === lastJson) return;
  lastJson = json;
  globalThis.wings.render(json);
}

const host = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: true,
  supportsMicrotasks: true,
  scheduleMicrotask: (fn) => Promise.resolve().then(fn),
  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,
  noTimeout: -1,
  getRootHostContext: () => ({}),
  getChildHostContext: (context) => context,
  createInstance(type, props) {
    const instance = { id: nextId++, type, props: {}, children: [] };
    setProps(instance, props);
    return instance;
  },
  createTextInstance: (text) => ({ id: nextId++, type: "#text", text }),
  appendInitialChild: (parent, child) => parent.children.push(child),
  appendChild: (parent, child) => parent.children.push(child),
  appendChildToContainer: (container, child) => container.children.push(child),
  removeChild: (parent, child) => parent.children.splice(parent.children.indexOf(child), 1),
  removeChildFromContainer: (container, child) => container.children.splice(container.children.indexOf(child), 1),
  insertBefore: (parent, child, before) => parent.children.splice(parent.children.indexOf(before), 0, child),
  insertInContainerBefore: (container, child, before) => container.children.splice(container.children.indexOf(before), 0, child),
  commitUpdate: (instance, _type, _old, props) => setProps(instance, props),
  commitTextUpdate: (text, _old, next) => {
    text.text = next;
  },
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  prepareForCommit: () => null,
  resetAfterCommit: commit,
  clearContainer: (container) => {
    container.children = [];
  },
  getPublicInstance: (instance) => instance,
  preparePortalMount() {},
  detachDeletedInstance() {},
  getCurrentUpdatePriority: () => priority,
  setCurrentUpdatePriority: (next) => {
    priority = next;
  },
  resolveUpdatePriority: () => priority || DefaultEventPriority,
  maySuspendCommit: () => false,
  preloadInstance: () => true,
  startSuspendingCommit() {},
  suspendInstance() {},
  waitForCommitToBeReady: () => null,
  NotPendingTransition: null,
  HostTransitionContext: createContext(null),
  resetFormInstance() {},
  requestPostPaintCallback() {},
  shouldAttemptEagerTransition: () => false,
  trackSchedulerEvent() {},
  resolveEventType: () => null,
  resolveEventTimeStamp: () => -1.1,
  hideInstance() {},
  unhideInstance() {},
  hideTextInstance() {},
  unhideTextInstance() {},
};

const reconciler = Reconciler(host);

// With no console to print to, an error the page doesn't catch is thrown again from a timer, where the app's loop
// sees it as the page's error.
function raise(error) {
  setTimeout(() => {
    throw error instanceof Error ? error : new Error(String(error));
  }, 0);
}

let container = null;

/**
 * Shows `element` as the page. Calling it again renders the new element in its place, like `showTab` does. `fill` lays
 * the page out edge to edge with no padding, as `showTab(element, { fill: true })` does; the tree's root carries it.
 */
export function mount(element, { fill = false } = {}) {
  root.props = fill ? { fill: true } : {};
  container ??= reconciler.createContainer(root, ConcurrentRoot, null, false, null, "", raise, () => {}, () => {}, () => {});
  // The first tree is ready when this returns, not on the next turn of the timer loop.
  reconciler.updateContainerSync(element, container, null, null);
  reconciler.flushSyncWork();
}

/**
 * Runs the handler `handlerId` names, with `payloadJson` (JSON, or nothing) merged into the event it gets, and renders
 * what it changed before returning. False when no such handler is on the page, like one whose button just went away.
 */
globalThis.__wingsEvent = (handlerId, payloadJson) => {
  const colon = handlerId.indexOf(":");
  const fn = shown.get(Number(handlerId.slice(0, colon)))?.fns?.[handlerId.slice(colon + 1)];
  if (typeof fn !== "function") return false;
  const event = { type: "click", preventDefault() {}, stopPropagation() {}, ...(payloadJson ? JSON.parse(payloadJson) : {}) };
  reconciler.discreteUpdates(fn, event);
  reconciler.flushSyncWork();
  return true;
};

const has = (node) => node !== undefined && node !== null && node !== false;
const slot = (name, node) => (has(node) ? h("Slot", { name }, node) : null);
const pick = (props, names) => {
  const out = {};
  for (const name of names) if (props[name] !== undefined) out[name] = props[name];
  return out;
};

/** A page that fills a tab: a `toolbar`, usually a `Toolbar`, over a body. `flush` takes the body's padding away. */
export function Page({ toolbar, flush, children }) {
  return h("Page", pick({ flush }, ["flush"]), slot("toolbar", toolbar), slot("body", children));
}
/** A page's top row: a `title` on the left, `children` like filters after it, and `actions` on the right. */
export function Toolbar({ title, actions, children }) {
  return h("Toolbar", null, slot("title", title), slot("children", children), slot("actions", actions));
}
/** Children one under the other. With `maxWidth`, a column at most that many px wide, centred in the room it has. */
export function Stack(props) {
  return h("Stack", pick(props, ["gap", "grow", "width", "maxWidth"]), props.children);
}
/** Children side by side, centred. */
export function Row(props) {
  return h("Row", pick(props, ["gap", "align", "justify", "wrap", "grow"]), props.children);
}
/** Text with a tone and size. */
export function Text(props) {
  return h("Text", pick(props, ["tone", "size", "weight", "mono", "numeric", "truncate", "clamp", "grow", "onClick"]), props.children);
}
/** A button. `onClick` gets an event with `preventDefault` and `stopPropagation`, which do nothing here. `busy` shows a spinner and ignores clicks. */
export function Button(props) {
  const { busy } = props;
  const label = props["aria-label"];
  return h("Button", { ...pick(props, ["variant", "size", "chip", "pressed", "onClick", "onKeyDown", "disabled"]), ...(label && { label }), ...(busy && { disabled: true }) }, busy ? h(Spinner, null) : null, props.children);
}
/** A small quiet button; with `pressed` it is on or off, drawn as a chip of `Chips` is. */
export function Chip({ pressed, onClick, disabled, children, ...rest }) {
  return h(Button, { chip: true, pressed: pressed === true, onClick, disabled, "aria-label": rest["aria-label"] }, children);
}
/** A short message in a tinted box. */
export function Notice({ tone, children }) {
  return h("Notice", pick({ tone }, ["tone"]), children);
}
/** A spinner. With a label it is announced by the app. */
export function Spinner(props) {
  return h("Spinner", pick(props, ["size", "label"]));
}
/** A checkbox with its label. `onChange` gets its next checked state. */
export function Checkbox(props) {
  const { onChange } = props;
  return h("Checkbox", { ...pick(props, ["checked", "disabled"]), ...(onChange && { onChange: (event) => onChange(event.checked) }) }, props.children);
}
/** An in-page confirmation card. Failed actions leave the card open and show their error. */
export function Confirm({ title, children, actions, cancelLabel = "Cancel", onCancel }) {
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(null);
  const run = (action) => async () => {
    setBusy(action.id);
    setError(null);
    try {
      await action.onConfirm();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(null);
    }
  };
  return h(
    "Confirm",
    null,
    slot("title", title),
    slot("body", error ? h(Notice, { tone: "danger" }, error) : children),
    h(
      "Slot",
      { name: "actions" },
      ...actions.map((action) => h(Button, { key: action.id, variant: action.variant ?? "destructive", disabled: busy !== null, onClick: run(action) }, action.label)),
      h(Button, { variant: "ghost", disabled: busy !== null, onClick: onCancel }, cancelLabel),
    ),
  );
}
/** A badge, like a label on a command or a branch name. */
export function Badge(props) {
  return h("Badge", pick(props, ["variant", "tone", "size"]), props.children);
}
/** Nothing to show: what's missing, a `description` on what to do about it, and `children` under them. */
export function Empty({ title, description, children }) {
  return h("Empty", null, slot("title", title), slot("description", description), slot("children", children));
}
/** What shows before the first data comes: "Loading…", unless `children` say otherwise. */
export function Loading({ children }) {
  return h("Loading", null, children);
}
/** A box around related content. */
export function Card(props) {
  return h("Card", null, props.children);
}
/** A section heading, with `actions` like chips or a button on its right. */
export function Heading(props) {
  const heading = h("Heading", null, props.children);
  return has(props.actions) ? h(Row, { justify: "between", wrap: true }, heading, props.actions) : heading;
}
/** A status dot. */
export function Dot(props) {
  const color = rgbaOf(props.color)?.[0];
  return h("Dot", { ...pick(props, ["tone"]), ...(color !== undefined && { color }) });
}
/** A labelled bar with its percentage; `note` goes under it, and `levels` are where it turns warn and danger. */
export function Gauge(props) {
  return h("Gauge", pick(props, ["label", "percent", "levels"]), slot("note", props.note || null));
}
/** A standalone progress bar. */
export function Meter(props) {
  return h("Meter", pick(props, ["label", "percent", "tone"]));
}
export function Board({ label, children }) {
  return h("Board", { label }, children);
}
export function BoardColumn({ title, count, actions, children }) {
  return h("BoardColumn", pick({ count }, ["count"]), slot("title", title), slot("actions", actions), slot("body", children));
}
export function BoardLanes({ label, columns, children }) {
  return h("Board", { label, lanes: true },
    slot("header", h("BoardRow", null, columns.map((column) => h(BoardColumn, { key: column.id, title: column.title, count: column.count })))),
    slot("body", children));
}
export function BoardLane({ label, title, count, actions, open, defaultOpen = true, onOpenChange, children }) {
  const [own, setOwn] = useState(defaultOpen);
  const shown = open ?? own;
  return h(Stack, { gap: "s" },
    h(Row, { gap: "s" }, h(Button, { variant: "ghost", "aria-label": label, onClick: () => {
      if (open === undefined) setOwn(!shown);
      onOpenChange?.(!shown);
    } }, shown ? "▾" : "▸"), title ?? h(Text, { weight: "strong" }, label),
    count === undefined ? null : h(Badge, { variant: "secondary" }, count), actions),
    shown ? h("BoardRow", null, children) : null);
}
export function BoardCell({ label, children }) {
  return h("BoardColumn", { label, cell: true }, slot("body", children));
}
/** A list of rows. */
export function List(props) {
  return h("List", null, props.children);
}
/** A row in a `List`. */
export function ListItem(props) {
  return h("ListItem", null, props.children);
}
/** More behind a `summary` line that opens it. */
export function Disclosure({ summary, open, onOpenChange, children }) {
  // Without `open` the app folds it on its own; with it, the page decides and hears each click.
  const props = open === undefined ? null : { open, onOpenChange: () => onOpenChange?.(!open) };
  return h("Disclosure", props, slot("summary", summary), slot("body", children));
}
/** A select of `options`. The app sends the value picked in the event's `value`, and `onChange` gets that value. */
export function Select(props) {
  const { onChange } = props;
  return h("Select", { ...pick(props, ["label", "options", "value"]), ...(onChange && { onChange: (event) => onChange(event.value) }) });
}

export function Chips(props) {
  return h("Chips", { ...pick(props, ["label", "options", "value"]), onChange: (event) => props.onChange(event.value) });
}

export function ListButton(props) {
  return h("ListButton", pick(props, ["onClick", "onKeyDown", "disabled", "align", "layout", "title", "indent", "aria-expanded"]), props.children);
}

export function SearchField(props) {
  return h("SearchField", { ...pick(props, ["label", "placeholder", "defaultValue"]), onInput: (event) => props.onInput?.({ ...event, currentTarget: { value: event.value }, target: { value: event.value } }) });
}

/** Only the requested window crosses the engine boundary, never the full history. */
export function Graph(props) {
  return h("Graph", { ...pick(props, ["total", "lanes", "selected", "offset", "rows", "onRangeChange"]), onSelect: (event) => props.onSelect(event.index) });
}

const FieldName = createContext(undefined);
// A field's label and hint are 14px, as plugin-ui.css's `.w-field-label` and `.w-field-hint`.
const hintOf = (hint, error) => ((error ?? hint) ? h(Text, { size: "field", tone: error ? "danger" : "muted" }, error ?? hint) : null);

/** A labelled field with a `hint` or an `error` under it, for the input in `children`, which takes the label as its name. */
export function Field({ label, hint, error, inline, children }) {
  const name = h(Text, { weight: "medium", size: "field" }, label);
  return h(
    FieldName.Provider,
    { value: typeof label === "string" ? label : undefined },
    h(Stack, { gap: "s" }, inline ? h(Row, null, name, children) : name, inline ? null : children, hintOf(hint, error)),
  );
}
/** Several controls under one label, with a `hint` or an `error` under them. */
export function FieldGroup({ label, hint, error, children }) {
  return h(Stack, { gap: "m" }, h(Text, { weight: "medium", size: "field" }, label), children, hintOf(hint, error));
}

/** What `onChange` of an input is given: the app's event, with the text as `target.value`. */
const changed = (value) => ({ type: "change", preventDefault() {}, stopPropagation() {}, target: { value }, currentTarget: { value } });
const input = (type, props, extra) => {
  const { onChange } = props;
  return h(type, { ...pick(props, ["value", "placeholder", "disabled", "maxLength", "onBlur", "onKeyDown", "autoFocus", "selectOnFocus"]), ...extra, ...(onChange && { onChange: (event) => onChange(changed(event.value)) }) });
};
/** A one-line text input. Outside a `Field`, give it a `label`. */
export function TextInput(props) {
  const name = useContext(FieldName);
  return input("TextInput", props, { label: props.label ?? name });
}
/** A multi-line text input, `rows` high at first. Outside a `Field`, give it a `label`. */
export function Textarea(props) {
  const name = useContext(FieldName);
  return input("Textarea", props, { label: props.label ?? name, rows: props.rows });
}

/** Editing in place: Enter or blur saves once, Escape cancels, a failed save keeps the draft. */
export function InlineInput({ label, value, required, onSave, onDone, onChange, ...rest }) {
  const [draft, setDraft] = useState(value);
  const { run, busy, error, clear } = useAction();
  const open = useRef(true);
  const saving = useRef(false);
  const cancel = () => {
    if (!open.current || saving.current) return;
    open.current = false;
    onDone();
  };
  const save = async () => {
    if (!open.current || saving.current) return;
    const next = draft.trim();
    if (next === value || (required && !next)) return cancel();
    saving.current = true;
    const ok = await run(() => onSave(next));
    saving.current = false;
    if (ok) {
      open.current = false;
      onDone();
    }
  };
  return h(Stack, { gap: "xs" }, h(TextInput, {
    ...rest, label, value: draft, disabled: busy, autoFocus: true, selectOnFocus: true,
    onChange: (event) => { setDraft(event.currentTarget.value); clear(); onChange?.(event); },
    onBlur: save,
    onKeyDown: (event) => {
      if (event.key === "Enter") save();
      if (event.key === "Escape") cancel();
    },
  }), error ? h(Notice, { tone: "danger" }, error) : null);
}

const matches = (option, query) => `${option.label} ${option.detail ?? ""}`.toLowerCase().includes(query.toLowerCase());
const MOST_SUGGESTIONS = 8;
/** A text field with its matching `options` under it as chips: the app has no popup list yet. Picking one calls `onPick`. */
export function Combobox({ label, value, onInput, options, onPick, filter = matches, empty = "No matches.", placeholder, maxLength, disabled }) {
  const found = options.filter((option) => filter(option, value)).slice(0, MOST_SUGGESTIONS);
  // What the field holds is an option: nothing is left to pick.
  const done = found.length === 1 && found[0].value === value;
  return h(
    Stack,
    { gap: "s" },
    h(TextInput, { label, value, placeholder, maxLength, disabled, onChange: (event) => onInput?.(event.target.value) }),
    done ? null : found.length
      ? h(Row, { gap: "s", wrap: true }, found.map((option) => h(Chip, { key: option.value, onClick: () => onPick(option) }, option.label)))
      : value ? h(Text, { size: "small", tone: "muted" }, empty) : null,
  );
}

/** A button that shows its `items` under it when pressed: the app has no popup menu yet. */
export function Menu({ label, items, variant, disabled }) {
  const [open, setOpen] = useState(false);
  return h(
    Stack,
    { gap: "xs" },
    h(Button, { variant, disabled, onClick: () => setOpen(!open) }, label, " ▾"),
    open ? items.map((item) => h(Button, { key: item.id, variant: item.variant === "destructive" ? "destructive" : "ghost", onClick: () => (setOpen(false), item.onSelect()) }, item.label)) : null,
  );
}

/** One of Lucide's icons, in the colour of the text. */
export function Icon({ name }) {
  return h("Icon", { name });
}
/** Small figures side by side, label over value. */
export function Stats({ items }) {
  return h(
    Row,
    { gap: "l", wrap: true },
    items.map((item) => h(Stack, { key: item.label, gap: "none" }, h(Text, { tone: "muted", size: "tiny" }, item.label), h(Text, { tone: item.tone, weight: "medium" }, item.value))),
  );
}
/** Label and value pairs, one per line. */
export function Properties({ items }) {
  return h(
    Stack,
    { gap: "s" },
    items.map((item) => h(Row, { key: item.label, gap: "m", align: "baseline" }, h(Text, { tone: "muted", size: "small" }, item.label), h(Text, { size: "small", grow: true }, item.value))),
  );
}
/** How long ago `iso` was, after `prefix`. Nothing when `iso` isn't a date. */
export function Time({ iso, prefix, now }) {
  const text = ago(iso, now);
  return text ? h(Fragment, null, prefix, text) : null;
}

export { ago, languageOf };
/** Puts `text` on the clipboard, through the app, within 5 s of a click or key in the page (up to 1 MB), as a browser allows. */
export function copyText(text) {
  return globalThis.wings.copyText(text);
}
/** A page's or a card's title: 14px, or 15px with `size="large"`. `truncate` cuts it to one line. */
export function Title({ size, truncate, children }) {
  return h(Text, { size: size === "large" ? "large" : undefined, weight: "strong", truncate }, children);
}
/** A round avatar with the initials of `name`. */
export function Avatar({ name, size }) {
  return h("Avatar", { name, ...(size && { size }) });
}
/** A block that stands in for content that is loading. */
export function Skeleton({ width, height }) {
  return h("Skeleton", pick({ width, height }, ["width", "height"]));
}
/** Skeletons that stand in for a section, announced once as `label`. */
export function Skeletons({ label, children }) {
  return h(Stack, { gap: "s" }, h(Spinner, { label }), children);
}
/** A row of `SummaryChip`s under a page's header. */
export function Summary({ children }) {
  return h(Row, { gap: "m", wrap: true }, children);
}
/** One fact in a `Summary`: a button when it has `onClick`, a badge otherwise. */
export function SummaryChip({ tone, onClick, children }) {
  return onClick ? h(Button, { variant: "outline", onClick }, tone ? h(Text, { tone }, children) : children) : h(Badge, { variant: "outline", tone }, children);
}
/** A conversation or an activity feed. */
export function Timeline({ children }) {
  return h(Stack, { gap: "m" }, children);
}
/** An entry of a `Timeline`: `author`'s avatar, name and time `at`, an optional `tag` and `actions`, then `children`. */
export function TimelineCard({ author, at, tag, actions, children }) {
  return h(Card, null, h(Stack, { gap: "s" },
    h(Row, { gap: "s" }, h(Avatar, { name: author, size: "small" }), h(Text, { weight: "strong" }, author),
      tag ? h(Badge, { variant: "secondary" }, tag) : null, at ? h(Text, { tone: "muted", size: "small", grow: true }, h(Time, { iso: at })) : null, actions),
    children));
}
/** A quiet one-line entry of a `Timeline`: a small avatar, the author in bold, `children`, then the time. */
export function TimelineNote({ author, at, children }) {
  return h(Row, { gap: "s", wrap: true }, h(Avatar, { name: author, size: "small" }), h(Text, { weight: "strong" }, author),
    h(Text, { tone: "muted" }, children), at ? h(Text, { tone: "muted", size: "small" }, h(Time, { iso: at })) : null);
}
/** Tabs that pick one of `items` over the panel they show, `children`: the chips of `Chips`. */
export function Tabs({ label, items, value, onChange, children }) {
  return h(Stack, { gap: "m" }, h(Chips, { label, options: items, value, onChange }), children);
}
/** A `list` pane and a `detail` pane side by side. */
export function ListDetail({ list, detail, empty, listWidth = 280 }) {
  return h(Row, { gap: "l", align: "start" }, h(Stack, { width: listWidth }, list), h(Stack, { grow: true }, has(detail) ? detail : (empty ?? h(Empty, { title: "Nothing selected" }))));
}

const openLink = (href) => () => {
  globalThis.wings.openUrl(href).catch(() => {});
};
/** The spans of a line of Markdown: a nested `Text` for each look, a link that opens in the browser. */
function mdSpans(text) {
  return parseInline(text).map((part, i) => {
    switch (part.kind) {
      case "code":
        return h(Text, { key: i, mono: true }, part.text);
      case "strong":
        return h(Text, { key: i, weight: "strong" }, emojify(part.text));
      case "del":
        return h(Text, { key: i, tone: "muted" }, emojify(part.text));
      case "image":
        return part.alt ? h(Fragment, { key: i }, part.alt) : null;
      case "link":
        return h(Text, { key: i, onClick: openLink(part.href) }, emojify(part.text));
      case "em":
        return h(Fragment, { key: i }, emojify(part.text));
      default:
        return h(Fragment, { key: i }, part.text);
    }
  });
}
function mdBlocks(blocks) {
  return blocks.map((b, i) => {
    switch (b.kind) {
      case "details":
        return h(Disclosure, { key: i, summary: mdSpans(b.summary) }, mdBlocks(b.blocks));
      case "heading":
        return h(Text, { key: i, weight: "strong", size: b.level <= 2 ? "large" : undefined }, mdSpans(b.text));
      case "list":
        return h(
          Stack,
          { key: i, gap: "xs" },
          b.items.map((item, j) =>
            h(
              Row,
              { key: j, gap: "s", align: "start" },
              item.depth ? h(Stack, { width: item.depth * 16 }) : null,
              h(Text, { tone: "muted" }, item.checked === null ? (b.ordered ? `${j + 1}.` : "•") : item.checked ? "☑" : "☐"),
              h(Text, { grow: true }, mdSpans(item.text)),
            ),
          ),
        );
      case "code":
        return h(CodeBlock, { key: i, code: b.text, lang: b.lang?.split(/\s/)[0] || undefined, title: b.lang || undefined });
      case "quote":
        return h(Row, { key: i, gap: "s", align: "start" }, h(Text, { tone: "muted" }, "▎"), h(Text, { tone: "muted", grow: true }, mdSpans(b.text)));
      case "rule":
        return h(Text, { key: i, tone: "muted" }, "────────────────");
      case "table":
        return h(
          Stack,
          { key: i, gap: "xs" },
          h(Row, { gap: "m" }, b.head.map((cell, j) => h(Text, { key: j, weight: "strong", grow: true }, mdSpans(cell)))),
          b.rows.map((row, r) => h(Row, { key: r, gap: "m", align: "start" }, row.map((cell, j) => h(Text, { key: j, grow: true }, mdSpans(cell))))),
        );
      default:
        return h(Text, { key: i }, mdSpans(b.text));
    }
  });
}
/** Markdown as nodes, drawn from the same parser as `wings-plugin-ui`'s. No HTML is parsed; images show as their alt text. */
export function Markdown({ source, inline }) {
  return inline ? h(Text, null, mdSpans(source)) : h(Stack, { gap: "m" }, mdBlocks(parseBlocks(source)));
}
/** Code, from `code` or from `lines` of `{ text, n?, kind?, highlighted? }`, in a card. It isn't colored by language. */
export function CodeBlock({ code, lines, title, label, busy }) {
  const rows = lines ?? String(code ?? "").split("\n").map((text) => ({ text }));
  return h(
    Card,
    null,
    h(
      Stack,
      { gap: "none" },
      title ? h(Text, { tone: "muted", size: "tiny" }, title) : null,
      busy ? h(Spinner, { label: label ?? "Loading" }) : null,
      rows.map((row, i) =>
        h(
          Row,
          { key: i, gap: "m", align: "baseline" },
          row.n === undefined ? null : h(Text, { mono: true, size: "tiny", tone: "muted", numeric: true }, String(row.n)),
          h(Text, { mono: true, size: "small", grow: true, tone: row.kind === "added" ? "success" : row.kind === "removed" ? "danger" : undefined, weight: row.highlighted ? "strong" : undefined }, row.text || " "),
        ),
      ),
    ),
  );
}

/** The project the page is about, read once: `undefined` until the app says, then the project, or `null` when none is open. */
export function useProject() {
  const [project, setProject] = useState(undefined);
  useEffect(() => {
    globalThis.wings.project().then(setProject, () => setProject(null));
  }, []);
  return project;
}
/** What `wings.openView` opened the tab with, as `check` reads it, or `null` when there were no args or `check` returns `null` or `undefined` for them. Read once. */
export function useViewArgs(check) {
  const [args] = useState(() => {
    const raw = globalThis.wings.viewArgs();
    return raw === null ? null : (check(raw) ?? null);
  });
  return args;
}
/** Shows `children(project)` once the app says which project the page is about, keyed by the project's id. */
export function ProjectGate({ loading = "Loading…", empty, children }) {
  const project = useProject();
  if (project === undefined) return h(Loading, null, loading);
  if (project === null) return h(Empty, { title: empty?.title ?? "No project", description: empty?.description });
  return h(Fragment, { key: project.id }, children(project));
}
function useSaved(read, write, initial, check) {
  const [value, setValue] = useState(null);
  useEffect(() => {
    void read().catch(() => null).then((raw) => setValue((raw == null ? undefined : check(raw)) ?? initial));
  }, []);
  return [value, (next) => {
    setValue(next);
    write(next).catch(() => {});
  }];
}
export const useTabState = (initial, check) => useSaved(() => wings.tabState(), (state) => wings.setTabState(state), initial, check);
export const useBlockState = (initial, check) => useSaved(() => wings.blockState(), (state) => wings.setBlockState(state), initial, check);
/** Runs the page's async actions and keeps what its buttons and error card show, as `useAction` of `wings-plugin-ui` does. */
export function useAction() {
  const [running, setRunning] = useState(0);
  const [error, setError] = useState(null);
  const run = useCallback(async (fn) => {
    setRunning((n) => n + 1);
    setError(null);
    try {
      await fn();
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    } finally {
      setRunning((n) => n - 1);
    }
  }, []);
  const clear = useCallback(() => setError(null), []);
  return { run, busy: running > 0, error, clear };
}

/** `[0xRRGGBB, alpha]` of a CSS `#rrggbb`, `rgb(r, g, b)` or `rgba(r, g, b, a)` colour; undefined for any other. */
function rgbaOf(css) {
  const hex = /^#([0-9a-f]{6})$/i.exec(css ?? "");
  if (hex) return [parseInt(hex[1], 16), 1];
  const rgb = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(css ?? "");
  if (!rgb || [rgb[1], rgb[2], rgb[3]].some((part) => Number(part) > 255)) return undefined;
  return [(Number(rgb[1]) << 16) | (Number(rgb[2]) << 8) | Number(rgb[3]), rgb[4] === undefined ? 1 : Math.min(1, Number(rgb[4]))];
}

/** A strip across its parent, like a page's header or footer, with a hairline on its `edge`. */
export function Bar({ edge, gap, children }) {
  return h("Bar", pick({ edge, gap }, ["edge", "gap"]), children);
}
/** A column that scrolls when its content is taller than the room it has, and takes what is left of its parent. */
export function Scroll({ children }) {
  return h("Scroll", null, children);
}
/** Two panes side by side: `start` is `defaultSize` points wide and `end` takes the rest. The app has no handle to drag yet. */
export function Split({ start, end, defaultSize }) {
  return h("Split", pick({ defaultSize }, ["defaultSize"]), slot("start", start), slot("end", end));
}
/** Wedges in rings round a centre with two lines of text in it, for a disk map. The page lays the wedges out. */
export function Sunburst(props) {
  const { rings, wedges, selected, title, detail, up, onPoint, onSelect } = props;
  return h("Sunburst", {
    rings,
    selected,
    title,
    detail,
    up,
    wedges: wedges.map((wedge) => {
      const [rgb, alpha] = rgbaOf(wedge.color) ?? [0, 1];
      return [wedge.from, wedge.to, wedge.ring, rgb, alpha * (wedge.alpha ?? 1)];
    }),
    ...(onPoint && { onPoint: (event) => onPoint(event.index) }),
    ...(onSelect && { onSelect: (event) => onSelect(event.index, { meta: event.meta === true }) }),
  });
}
/** Calls `load` when the component mounts and each time it changes, then every `ms` while the app is focused. */
export function usePoll(load, ms) {
  useEffect(() => {
    if (globalThis.wings.focused) load();
    const timer = setInterval(() => {
      if (globalThis.wings.focused) load();
    }, ms);
    return () => clearInterval(timer);
  }, [load, ms]);
}
