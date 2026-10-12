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
import { createContext, createElement as h, useContext, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

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

// ---------- talking to main ----------

const HELLO = { type: "hello" };

/**
 * The newest message of `type` the plugin's other pages broadcast, or null until one comes. Sends `hello` once
 * when the component mounts, so main answers with its state; pass null to send nothing.
 */
export function useMainState(type, hello = HELLO) {
  const [state, setState] = useState(null);
  useEffect(() => {
    const off = wings.onBroadcast((message) => {
      if (message?.type === type) setState(message);
    });
    if (hello != null) void wings.broadcast(hello);
    return off;
    // `hello` is only sent when the component mounts.
  }, [type]);
  return state;
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

/** One of Lucide's icons, as the app's own components draw them. */
const icon = (path, className) => h("svg", { className, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true }, h("path", { d: path }));
const CHEVRON_DOWN = "m6 9 6 6 6-6";
const CHECK = "M20 6 9 17l-5-5";

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
      h(BaseSelect.Icon, { render: icon(CHEVRON_DOWN, "w-select-icon") }),
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
              h(BaseSelect.Item, { key: o.value, value: o.value, label: o.label, disabled: o.disabled, className: "w-select-item" }, h(BaseSelect.ItemText, null, o.label), h(BaseSelect.ItemIndicator, { className: "w-item-check", render: h("span") }, icon(CHECK))),
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
    h(BaseCheckbox.Root, { "aria-invalid": group?.invalid || undefined, ...rest, id: useFieldId(id), nativeButton: true, render: h("button", { type: "button" }), onCheckedChange: onChange && ((checked) => onChange(checked)), className: "w-checkbox" }, h(BaseCheckbox.Indicator, { className: "w-checkbox-indicator", render: h("span") }, icon(CHECK))),
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
