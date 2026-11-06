// wings-plugin-ui/native: the components of ui.js that the native Wings app draws, rendered by React into a JSON tree
// instead of into a DOM. The page runs in a JS engine with no document, and the app turns each tree into its own
// controls. Handlers never leave the page: the tree names them by id, and the app sends the id back when one fires.
import { createContext, createElement as h, Fragment, useCallback, useContext, useEffect, useState } from "react";
import Reconciler from "react-reconciler";
import { ConcurrentRoot, DefaultEventPriority } from "react-reconciler/constants.js";
import { errorMessage } from "wings-plugin-sdk/async";
import { ago } from "wings-plugin-sdk/format";

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

/** Shows `element` as the page. Calling it again renders the new element in its place, like `showTab` does. */
export function mount(element) {
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
/** Children one under the other. */
export function Stack(props) {
  return h("Stack", pick(props, ["gap", "grow"]), props.children);
}
/** Children side by side, centred. */
export function Row(props) {
  return h("Row", pick(props, ["gap", "align", "justify", "wrap", "grow"]), props.children);
}
/** Text with a tone and size. */
export function Text(props) {
  return h("Text", pick(props, ["tone", "size", "weight", "mono", "numeric", "truncate", "clamp", "grow"]), props.children);
}
/** A button. `onClick` gets an event with `preventDefault` and `stopPropagation`, which do nothing here. `busy` shows a spinner and ignores clicks. */
export function Button(props) {
  const { busy } = props;
  const label = props["aria-label"];
  return h("Button", { ...pick(props, ["variant", "onClick", "disabled"]), ...(label && { label }), ...(busy && { disabled: true }) }, busy ? h(Spinner, null) : null, props.children);
}
/** A small quiet button; with `pressed` it is on or off, drawn as a chip of `Chips` is. */
export function Chip({ pressed, onClick, disabled, children, ...rest }) {
  return h(Button, { variant: pressed ? "secondary" : "ghost", onClick, disabled, "aria-label": rest["aria-label"] }, children);
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
/** A section heading. */
export function Heading(props) {
  return h("Heading", null, props.children);
}
/** A status dot. */
export function Dot(props) {
  return h("Dot", pick(props, ["tone"]));
}
/** A labelled bar with its percentage; `note` goes under it, and `levels` are where it turns warn and danger. */
export function Gauge(props) {
  return h("Gauge", pick(props, ["label", "percent", "levels"]), slot("note", props.note || null));
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
export function Disclosure({ summary, children }) {
  return h("Disclosure", null, slot("summary", summary), slot("body", children));
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
  return h("ListButton", pick(props, ["onClick", "disabled", "align", "layout", "title", "indent", "aria-expanded"]), props.children);
}

export function SearchField(props) {
  return h("SearchField", { ...pick(props, ["label", "placeholder", "defaultValue"]), onInput: (event) => props.onInput?.({ ...event, currentTarget: { value: event.value }, target: { value: event.value } }) });
}

/** Only the requested window crosses the engine boundary, never the full history. */
export function Graph(props) {
  return h("Graph", { ...pick(props, ["total", "lanes", "selected", "offset", "rows", "onRangeChange"]), onSelect: (event) => props.onSelect(event.index) });
}

const FieldName = createContext(undefined);
const hintOf = (hint, error) => ((error ?? hint) ? h(Text, { size: "small", tone: error ? "danger" : "muted" }, error ?? hint) : null);

/** A labelled field with a `hint` or an `error` under it, for the input in `children`, which takes the label as its name. */
export function Field({ label, hint, error, inline, children }) {
  const name = h(Text, { weight: "medium" }, label);
  return h(
    FieldName.Provider,
    { value: typeof label === "string" ? label : undefined },
    h(Stack, { gap: "s" }, inline ? h(Row, null, name, children) : name, inline ? null : children, hintOf(hint, error)),
  );
}
/** Several controls under one label, with a `hint` or an `error` under them. */
export function FieldGroup({ label, hint, error, children }) {
  return h(Stack, { gap: "m" }, h(Text, { weight: "medium" }, label), children, hintOf(hint, error));
}

/** What `onChange` of an input is given: the app's event, with the text as `target.value`. */
const changed = (value) => ({ type: "change", preventDefault() {}, stopPropagation() {}, target: { value }, currentTarget: { value } });
const input = (type, props, extra) => {
  const { onChange } = props;
  return h(type, { ...pick(props, ["value", "placeholder", "disabled", "maxLength"]), ...extra, ...(onChange && { onChange: (event) => onChange(changed(event.value)) }) });
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

/** The project the page is about, read once: `undefined` until the app says, then the project, or `null` when none is open. */
export function useProject() {
  const [project, setProject] = useState(undefined);
  useEffect(() => {
    globalThis.wings.project().then(setProject, () => setProject(null));
  }, []);
  return project;
}
/** What `wings.openView` opened the tab with. The app opens a native page without arguments, so it is always `null`. */
export function useViewArgs() {
  return null;
}
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
