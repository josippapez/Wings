/// <reference types="wings-plugin-sdk" />
// wings-plugin-ui: say what a page shows and where, and Wings decides how it looks. Every component renders
// `w-*` class names that Wings' plugin-ui.css styles, the interactive ones on Base UI's components, drawn as the app's
// own shadcn/ui ones. Your own components and CSS work too.
import type { ComponentPropsWithoutRef, JSX, ReactElement, ReactNode, Ref } from "react";

import type { ShellOptions } from "wings-plugin-sdk/shell";

// ---------- places ----------

/** Shows `element`, any React element, as the page of a sidebar. One call per page; calling again renders the new element. */
export function showSidebar(element: ReactElement, options?: ShellOptions): void;
/** Shows `element` as a view's page, in a modal or a window of its own. */
export function showView(element: ReactElement, options?: ShellOptions): void;
/** Shows `element` as a view's page in a tab next to the terminals. */
export function showTab(element: ReactElement, options?: ShellOptions): void;
/** Shows `element` as a sidebar's popover. */
export function showPopover(element: ReactElement, options?: ShellOptions): void;
/** Shows `element` as the plugin's page in Wings Settings. */
export function showSettings(element: ReactElement, options?: ShellOptions): void;

// ---------- talking to main ----------

/**
 * The newest message of `type` the plugin's other pages broadcast, or null until one comes. Sends `hello`
 * (`{ type: "hello" }` by default) once when the component mounts, so main answers with its state; pass null to
 * send nothing.
 */
export function useMainState<T extends { type: string }>(type: T["type"], hello?: unknown): T | null;

// ---------- shared ----------

/** A colour that says something. */
export type Tone = "muted" | "info" | "success" | "warn" | "danger";
type Props<Tag extends keyof JSX.IntrinsicElements> = ComponentPropsWithoutRef<Tag>;
type As = "div" | "span" | "p" | "section" | "header" | "footer" | "strong" | "time" | "label" | "li";
/** The elements `Row` and `Stack` can be. */
type BoxAs = As | "form" | "ul" | "ol";
/** The elements `Card` can be. */
type CardAs = As | "form" | "article" | "ul" | "ol";
/** The props of element `E`, with `own` on top: where they share a name, `own` wins. */
type Own<E extends keyof JSX.IntrinsicElements, own> = Omit<Props<E>, keyof own> & own;
/** Spacing between children: none, xs (2px), s (4px), m (8px, the default) or l (12px), set by Wings. */
export type Gap = "none" | "xs" | "s" | "m" | "l";

// ---------- layout and text ----------

/**
 * Children side by side, centred. `as` picks the element, like a `form` or a `ul`; the props and handlers are the
 * element's, so `<Row as="form" onSubmit={(e) => e.currentTarget.requestSubmit()}>` has a `HTMLFormElement`.
 */
export function Row<E extends BoxAs = "div">(props: Own<E, { as?: E; gap?: Gap; align?: "start" | "baseline"; justify?: "end" | "between"; wrap?: boolean; grow?: boolean }>): ReactElement;
/** Children one under the other. As a `ul` or `ol` it's a list with no bullets, of `li` children. `as` types the props like `Row`'s. */
export function Stack<E extends BoxAs = "div">(props: Own<E, { as?: E; gap?: Gap; grow?: boolean }>): ReactElement;
/** Text with a tone and size, that can `grow` to fill a row or `truncate` with an ellipsis. */
export function Text(
  props: Props<"span"> & {
    as?: As;
    tone?: Tone;
    /** small is 12px, tiny 11px. */
    size?: "small" | "tiny";
    weight?: "medium" | "strong";
    mono?: boolean;
    /** Digits of equal width, for figures that change. */
    numeric?: boolean;
    /** One line, cut with an ellipsis. */
    truncate?: boolean;
    /** Up to three lines. */
    clamp?: boolean;
    grow?: boolean;
  },
): ReactElement;
/** A section heading, an `h2`, with `actions` like chips or a button on its right. */
export function Heading(props: Props<"h2"> & { level?: 2 | 3 | 4; actions?: ReactNode }): ReactElement;
/** A page's or a card's title, like a sprint's name or a setup step's question: an `h1` unless `as` says otherwise. */
/** `truncate` cuts a title that's too long for its place to one line, with an ellipsis. */
export function Title(props: Props<"h1"> & { as?: "h1" | "h2" | "h3"; size?: "large"; truncate?: boolean }): ReactElement;
/** A card. `compact` for a summary line. As a `form` it holds a form; as a `ul` or `ol`, a list of `li`. `as` types the props like `Row`'s. */
export function Card<E extends CardAs = "div">(props: Own<E, { as?: E; compact?: boolean }>): ReactElement;

// ---------- figures ----------

/** A bar from 0 to 100, with the meter role. */
export function Meter(props: Props<"div"> & { percent: number; label: string; tone?: "warn" | "danger" }): ReactElement;
/** A labelled bar with its percentage and a `note` under it. `levels` are where it turns warn and danger, like `[75, 90]`. */
export function Gauge(props: Props<"div"> & { label: string; percent: number; note?: ReactNode; tone?: "warn" | "danger"; levels?: [number, number] }): ReactElement;
export interface Stat {
  label: string;
  value: ReactNode;
  tone?: "warn" | "danger";
  title?: string;
}
/** Small figures side by side, label over value. */
export function Stats(props: Props<"dl"> & { items: Stat[] }): ReactElement;
/** Label and value pairs, one per line. */
export function Properties(props: Props<"dl"> & { items: { label: string; value: ReactNode }[] }): ReactElement;

// ---------- lists ----------

/** A list of rows: a `ul`, or an `ol` when `ordered`. `compact` rows have no space between them, for long lists. */
export function List(props: Props<"ul"> & { ordered?: boolean; compact?: boolean }): ReactElement;
/** A row in a `List`. Holding a `ListButton`, the button is the row. */
export function ListItem(props: Props<"li"> & { align?: "start" }): ReactElement;
/** A row that's a button, inside a `ListItem`. */
export function ListButton(props: Props<"button"> & { align?: "baseline" }): ReactElement;

// ---------- status ----------

/** A status dot, green unless `tone` says otherwise. With a `label` it's announced; otherwise it's decoration. */
export function Dot(
  props: Props<"span"> & {
    tone?: Exclude<Tone, "success">;
    /** A colour from your data, like a work item type's `#cc293d`, set as `--w-color`. Wins over `tone`. */
    color?: string;
    label?: string;
  },
): ReactElement;
/** A spinner, 16px like the app's, in the colour of the text around it. With a `label` it's announced; otherwise it's decoration. */
export function Spinner(props: Props<"span"> & { size?: "large"; label?: string }): ReactElement;
/** shadcn/ui's Badge variants. */
export type BadgeVariant = "default" | "secondary" | "destructive" | "outline" | "ghost" | "link";
/** A badge, like a label on a command or a branch name, drawn as the app's Badge. `tone` is Wings' own: a colour that says something. */
export function Badge(props: Props<"span"> & { variant?: BadgeVariant; tone?: Exclude<Tone, "muted"> }): ReactElement;
/** What shows before the first data comes: "Loading…" unless `children` say otherwise. */
export function Loading(props: Props<"p">): ReactElement;
/** Nothing to show: what's missing, a `description` on what to do about it, and `children` under them, like a button. */
export function Empty(props: Omit<Props<"div">, "title"> & { title: ReactNode; description?: ReactNode }): ReactElement;

// ---------- buttons ----------

/** shadcn/ui's Button variants. */
export type ButtonVariant = "default" | "secondary" | "outline" | "ghost" | "destructive" | "link";
/** shadcn/ui's Button sizes. */
export type ButtonSize = "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";
/**
 * A button on Base UI's Button, `type="button"` unless you say otherwise. `variant` "default" is the filled one.
 * `busy` shows a spinner and ignores clicks while it keeps the keyboard focus, unlike `disabled`: it's `aria-busy`
 * and `aria-disabled`.
 */
export function Button(props: Props<"button"> & { variant?: ButtonVariant; size?: ButtonSize; busy?: boolean }): ReactElement;
/** A small quiet button. With `pressed` it's a toggle. */
export function Chip(props: Props<"button"> & { pressed?: boolean }): ReactElement;
export interface Option<V extends string> {
  value: V;
  label: ReactNode;
  title?: string;
}
/** Chips that pick one of `options`, like a filter, in a group named `label`. `onChange` gets each click, the chip that's on too. */
export function Chips<V extends string>(
  props: Omit<Props<"div">, "onChange"> & {
    label: string;
    /** Each chip's other props, like `id`, go to its button. */
    options: (Option<V> & Omit<Props<"button">, "value" | "onClick" | "children">)[];
    value: V;
    onChange: (value: V) => void;
  },
): ReactElement;
/**
 * Tabs named `label` over the panel they show, `children`, on Base UI's Tabs. Arrow keys, Home and End move between
 * tabs and show the one they land on. `variant` is shadcn/ui's: "default", a pill in a tray, or "line", an underline.
 */
export function Tabs<V extends string>(
  props: Omit<Props<"div">, "onChange"> & { label: string; items: Option<V>[]; value: V; onChange: (value: V) => void; variant?: "default" | "line" },
): ReactElement;

// ---------- fields ----------

/**
 * A labelled field with a `hint` or an `error` under it, for the `TextInput`, `SearchField`, `Select` or `Combobox` in
 * `children`, on Base UI's Field. It takes one input: Base UI gives every input in a Field the first one's id, so a
 * second input goes beside the Field.
 */
export function Field(
  props: Props<"div"> & {
    label: ReactNode;
    hint?: ReactNode;
    error?: ReactNode;
    /** The input's id, and the label's `htmlFor`, for a page that puts focus back by id. Generated otherwise. */
    id?: string;
    /** The label and the input on one line. */
    inline?: boolean;
  },
): ReactElement;
/** Several controls under one label (a `fieldset` and its `legend`), like checkboxes, with a `hint` or an `error` under them. */
export function FieldGroup(props: Props<"fieldset"> & { label: ReactNode; hint?: ReactNode; error?: ReactNode }): ReactElement;
/** A text input. Outside a `Field`, give it a `label`. */
export function TextInput(props: Props<"input"> & { label?: string; ref?: Ref<HTMLInputElement> }): ReactElement;
/** A search box. `label` names it for screen readers. */
export function SearchField(props: Omit<Props<"input">, "type"> & { label?: string; ref?: Ref<HTMLInputElement> }): ReactElement;
/**
 * A select of `options` on Base UI's Select: a button that opens a list in the page. `onChange` gets the value picked.
 * Without a `value` it starts on `defaultValue`, or the first option. Its props are the button's, not a `select`'s.
 */
export function Select<V extends string>(
  props: Omit<Props<"button">, "onChange" | "children" | "value" | "defaultValue"> & {
    label?: string;
    options: (Option<V> & { label: string; disabled?: boolean })[];
    value?: V;
    defaultValue?: V;
    onChange?: (value: V) => void;
    name?: string;
    required?: boolean;
    form?: string;
  },
): ReactElement;
/**
 * A checkbox with its label, `children`, on Base UI's Checkbox. `onChange` gets whether it's checked. Its element is
 * a `button` with the `checkbox` role, and `id` is that button's.
 */
export function Checkbox(
  props: Omit<Props<"button">, "onChange" | "type" | "children" | "value" | "defaultValue"> & {
    children?: ReactNode;
    checked?: boolean;
    defaultChecked?: boolean;
    onChange?: (checked: boolean) => void;
    name?: string;
    value?: string;
    required?: boolean;
    readOnly?: boolean;
  },
): ReactElement;
export interface ComboboxOption {
  value: string;
  label: string;
  /** A second, quieter part, like a project's client. */
  detail?: string;
}
/**
 * A text field with `options` under it, on Base UI's Combobox: the list is a popup in the page. Arrows move, Enter picks, Escape and Tab close, and pressing outside
 * closes it. With `freeText` the field keeps what you type (`value`, `onInput`) and starts with nothing
 * highlighted; otherwise you type to find an option and the field shows `value`, what you picked, once closed.
 * `filter` decides what matches what's typed: the label or the detail containing it, by default. Without
 * `freeText` the field's text is selected when it gets focus and after a pick, so typing replaces the label. An
 * `onKeyDown` you pass runs first; `preventDefault()` in it keeps the Combobox's own keys from running.
 */
export function Combobox(
  props: Omit<Props<"input">, "value" | "onInput" | "type"> & {
    label?: string;
    value: string;
    onInput?: (text: string) => void;
    options: ComboboxOption[];
    onPick: (option: ComboboxOption) => void;
    /** Enter with no option highlighted, with the text typed. The Enter doesn't submit a form around it. */
    onEnter?: (text: string) => void;
    freeText?: boolean;
    /** Open the list when the field gets focus, by default; false opens it on a click, a key or typing only, for a page that puts focus back by id after it renders. */
    openOnFocus?: boolean;
    /**
     * When the list is shown. `"auto"` (default): on focus, a click, a key or typing, and Escape, Tab, blur to another
     * element or a press outside close it. `"whileText"`: the list is shown exactly while the field has text and an
     * option matches it, whether or not the field has focus. Escape, Tab and a press outside only clear the highlighted
     * option (`aria-activedescendant`); emptying the text or picking closes it, and there's no `empty` message. A
     * list left open after the field loses focus is a screen reader caveat, see the README.
     */
    open?: "auto" | "whileText";
    filter?: (option: ComboboxOption, query: string) => boolean;
    /** What shows when nothing matches. */
    empty?: ReactNode;
  },
): ReactElement;
export interface MenuItem {
  id: string;
  label: ReactNode;
  onSelect: () => void;
  /** "destructive" draws it in the danger colour, like shadcn/ui's DropdownMenuItem. */
  variant?: "default" | "destructive";
}
/**
 * A button, showing `label`, that opens a menu of `items` under it, on Base UI's Menu: the menu is a popup in the
 * page. Arrows, Home and End move, Enter or Space picks, Escape closes and puts focus back on the button. `align` end
 * lines the menu up with the button's right. `variant` and `size` are the Button's.
 */
export function Menu(props: Omit<Props<"button">, "children"> & { label: ReactNode; items: MenuItem[]; align?: "end"; variant?: ButtonVariant; size?: ButtonSize }): ReactElement;
/** More behind a `summary` line that opens it, on Base UI's Collapsible. `open` is controlled: give it `onOpenChange`, or use `defaultOpen`. */
export function Disclosure(
  props: Omit<Props<"div">, "children"> & { summary: ReactNode; children?: ReactNode; open?: boolean; defaultOpen?: boolean; onOpenChange?: (open: boolean) => void },
): ReactElement;
