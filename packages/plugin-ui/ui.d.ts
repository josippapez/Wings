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
/**
 * Shows `element` as the page of a block on a Project page, like `showView`, and keeps the block as tall as the page:
 * each time the page's height changes it asks Wings for that height with `wings.setHeight`, which clamps it to the
 * block's `minHeight` and 1200. A page in `contributes.blocks` uses this in place of `showView`.
 */
export function showBlock(element: ReactElement): void;

// ---------- the page's project, state and actions ----------

/** The project the page is about: `undefined` until Wings says, then the project, or `null` when none is open. Follows `wings.onProject`. */
export function useProject(): WingsProject | null | undefined;
/**
 * Shows `children(project)` once Wings has said which project the page is about, keyed by the project's id, so a page
 * that holds state starts over for another project. Until then it shows `loading` ("Loading…" by default); with no
 * project, an `Empty` titled "No project" unless `empty` says otherwise.
 */
export function ProjectGate(props: {
  loading?: ReactNode;
  empty?: { title?: ReactNode; description?: ReactNode };
  children: (project: WingsProject) => ReactNode;
}): ReactElement;

/**
 * Turns what a page was given or saved, which is any JSON, into the value it expects. Returns `null` or `undefined` for
 * what isn't one, and is not called when there is nothing.
 */
export type Check<T> = (raw: unknown) => T | null | undefined;
/**
 * The state the tab saved with `wings.setTabState`, and a function that sets and saves it. The value is `null` until
 * it's read, then what `check` returns for the saved state, or `initial` when there is none or `check` returns
 * `null` or `undefined`. A save that fails, like one that is too big, leaves the page with the value it set.
 */
export function useTabState<T>(initial: T, check: Check<T>): [T | null, (next: T) => void];
/** Like `useTabState`, for a block: the state saved with `wings.setBlockState`. */
export function useBlockState<T>(initial: T, check: Check<T>): [T | null, (next: T) => void];
/**
 * What `wings.openView` opened this tab with, as `check` reads it, or `null` when there were no args or `check` returns
 * `null` or `undefined` for them. Read once: a tab opened with other args reloads.
 */
export function useViewArgs<T>(check: Check<T>): T | null;

export interface ActionState {
  /** Runs `fn`. It never rejects: if `fn` throws, `error` is its message. Resolves to whether `fn` worked. Starting one clears the error. */
  run(fn: () => unknown): Promise<boolean>;
  /** True while any `run` is going. */
  busy: boolean;
  /** The message of the last `run` that threw, until the next `run` starts or `clear` is called. */
  error: string | null;
  clear(): void;
}
/** Runs the page's async actions and keeps what its buttons and error card show: `busy` while one is going, and the message of the last one that threw as `error`. */
export function useAction(): ActionState;
/**
 * Calls `load` when the component mounts and each time `load` changes, if Wings is focused, then every `ms` while it is,
 * and once when Wings is focused again, so a block out of view asks for nothing. Wrap `load` in `useCallback` with what it reads, so it changes only when that does.
 */
export function usePoll(load: () => unknown, ms: number): void;
/** Puts `text` on the clipboard. A sandboxed frame may refuse the clipboard API, so then it uses a hidden textarea. */
export function copyText(text: string): Promise<void>;

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

// ---------- pages ----------

/**
 * A page that fills a tab: a `toolbar`, usually a `Toolbar`, over a body that scrolls. `flush` takes the body's padding
 * and scrolling away, for a body that lays itself out, like a `ListDetail`, a `Split` or a `Board`. Show it with
 * `showTab(<Page />, { fill: true })`.
 */
export function Page(props: Props<"div"> & { toolbar?: ReactNode; flush?: boolean }): ReactElement;
/** A page's top row: a `title` (cut to one line) on the left, `children` like filters or a search box after it, and `actions` on the right. */
export function Toolbar(props: Omit<Props<"header">, "title"> & { title?: ReactNode; actions?: ReactNode }): ReactElement;
/**
 * A `list` pane and a `detail` pane side by side. Without a `detail` the detail pane shows `empty`. When the page is
 * narrower than 560px it shows one pane: the list, or the detail once there is one, with a back button if `onBack`
 * is given. Each pane is a `section`; name them with `listLabel` and `detailLabel`.
 */
export function ListDetail(
  props: Omit<Props<"div">, "children"> & {
    list: ReactNode;
    detail?: ReactNode;
    /** What the detail pane shows without a `detail`: an `Empty` saying "Nothing selected" by default. */
    empty?: ReactNode;
    /** The list's width in px, 280 by default. */
    listWidth?: number;
    listLabel?: string;
    detailLabel?: string;
    /** Shows a back button above the detail when the page is too narrow for both panes. */
    onBack?: () => void;
    /** The back button's text, "Back" by default. */
    backLabel?: string;
  },
): ReactElement;
/** Columns side by side that scroll sideways, each a `BoardColumn`. A region named `label`, that takes keyboard focus so the arrow keys scroll it. */
export function Board(props: Omit<Props<"div">, "role"> & { label: string }): ReactElement;
/** A column of a `Board`: a `title` with the `count` of its cards and `actions`, over `children`, usually `Card`s, that scroll. */
export function BoardColumn(props: Omit<Props<"section">, "title"> & { title: ReactNode; count?: number; actions?: ReactNode }): ReactElement;
/** A column of `BoardLanes`: its header text and the count of its cards across every lane. */
export interface BoardLanesColumn {
  id: string;
  title: ReactNode;
  count?: number;
}
/**
 * A board in rows, like Linear's sub-grouping: the `columns` head it once, and each child is a `BoardLane`. The lanes
 * scroll down under the column header, and the whole board scrolls sideways when the columns don't fit. The lanes are a
 * region named `label` that takes keyboard focus, so the arrow keys scroll it.
 */
export function BoardLanes(props: Omit<Props<"div">, "role"> & { label: string; columns: BoardLanesColumn[] }): ReactElement;
/**
 * A row of `BoardLanes`: a header with a button that folds the row, its `title` (`label` when there's none), the `count`
 * of its cards and `actions`, over one `BoardCell` per column, in the columns' order. `open` is controlled: give it
 * `onOpenChange`, or use `defaultOpen` (open unless false).
 */
export function BoardLane(
  props: Omit<Props<"section">, "title"> & { label: string; title?: ReactNode; count?: number; actions?: ReactNode; open?: boolean; defaultOpen?: boolean; onOpenChange?: (open: boolean) => void },
): ReactElement;
/** A lane's cell under one column, a stack of `children`, usually `Card`s. `label` names it for a screen reader, like the column's title. */
export function BoardCell(props: Omit<Props<"div">, "role"> & { label: string }): ReactElement;
/**
 * Two panes, `start` and `end`, with a handle between them. Drag the handle, or focus it and press the arrow keys
 * (Shift for bigger steps), Home or End. A drag changes a CSS variable only and commits on release, so nothing renders
 * per frame. The handle is a `separator` with `aria-valuenow` in px.
 */
export function Split(
  props: Omit<Props<"div">, "children"> & {
    start: ReactNode;
    end: ReactNode;
    /** "horizontal" (default) puts the panes side by side, "vertical" one over the other. */
    direction?: "horizontal" | "vertical";
    /** The start pane's size in px at first, 280 by default. Read from `wings.tabState()` to restore a saved one. */
    defaultSize?: number;
    /** The smallest either pane gets, in px, 120 by default. */
    min?: number;
    /** The biggest the start pane gets, in px. */
    max?: number;
    /** Gets the start pane's size in px when a drag or a key press ends, to save it with `wings.setTabState()`. */
    onResize?: (size: number) => void;
    /** Names the handle: "Resize panes" by default. */
    label?: string;
  },
): ReactElement;

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
/**
 * A round avatar with the initials of `name` (up to two letters) on a colour that depends only on the name, so a person
 * looks the same everywhere. Decoration next to the name unless given a `label`, which names it for assistive tech.
 */
export function Avatar(props: Props<"span"> & { name: string; size?: "small" | "large"; label?: string }): ReactElement;
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

// ---------- markdown ----------

/**
 * Markdown as React elements: headings, paragraphs, lists (with read-only task checkboxes), fenced code, quotes,
 * rules, GFM tables, bold, italics, strikethrough, inline code, links, bare https URLs, images and emoji shortcodes like
 * `:white_check_mark:` (unknown names and anything in code stay as typed). No HTML is ever
 * parsed. Links open through `wings.openUrl` and only when they are https, so the manifest needs
 * `permissions.openUrl: ["https://"]` for them to work. Images load only from https or `data:image/` (png, jpeg, gif,
 * webp) URLs of up to 8,000 characters; anything else shows its alt text. A remote https image shows as a "Load image from <host>" button and loads only when clicked, since text an agent wrote could otherwise leak data in the URL; a data: image shows at once. `inline` renders only the inline parts, in
 * a span, for a table cell or a one-line value. Re-renders only when `source` changes.
 */
export function Markdown(props: Omit<Props<"div">, "children"> & { source: string; inline?: boolean }): ReactElement;
/** The longest `data:image/` URL, in characters, that `Markdown` shows. */
export const MAX_DATA_IMAGE: number;

// ---------- activity: times, a summary row, skeletons and a timeline ----------

/** "just now", "5 min ago", "3 h ago", "2 d ago", then the date; "" when `iso` isn't a date. `now` is for tests. */
export function ago(iso: string, now?: number): string;
/** A moment as a `time` element: how long ago (after `prefix`), the exact date and time as its title. Renders nothing when `iso` isn't a date. */
export function Time(props: Omit<Props<"time">, "dateTime" | "children"> & { iso: string; prefix?: ReactNode; now?: number }): ReactElement | null;
/** A row of `SummaryChip`s under a page's header. */
export function Summary(props: Props<"div">): ReactElement;
/** One fact in a `Summary`: a button when it has `onClick`, text otherwise. `tone` colours it. */
export function SummaryChip(props: Omit<Props<"span">, "onClick"> & { tone?: "info" | "success" | "warn" | "danger"; onClick?: Props<"button">["onClick"]; type?: "button" | "submit" }): ReactElement;
/** A block that stands in for loading content, `width` and `height` in px or any CSS length. Decoration: wrap a group in `Skeletons`. */
export function Skeleton(props: Props<"span"> & { width?: number | string; height?: number | string }): ReactElement;
/** A group of `Skeleton`s announced once as `label`, with `aria-busy`. */
export function Skeletons(props: Omit<Props<"div">, "role" | "aria-label"> & { label: string }): ReactElement;
/** An ordered list of `TimelineCard`s and `TimelineNote`s, named `label`. */
export function Timeline(props: Props<"ol"> & { label: string }): ReactElement;
/**
 * An entry of a `Timeline`: the `Avatar`, name and time of `author` and `at` (ISO), an optional `tag` and `actions`
 * at the right of that line, and `children` as the body. `quiet` for what a bot or the system wrote; `clamp` cuts a long
 * body until "Show more".
 */
export function TimelineCard(props: Omit<Props<"li">, "children"> & { author: string; at?: string; tag?: ReactNode; actions?: ReactNode; quiet?: boolean; clamp?: boolean; children?: ReactNode }): ReactElement;
/** A quiet one-line entry of a `Timeline`: a small avatar, the author in bold, `children`, then the time. */
export function TimelineNote(props: Omit<Props<"li">, "children"> & { author: string; at?: string; children?: ReactNode }): ReactElement;
