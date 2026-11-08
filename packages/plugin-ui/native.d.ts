// wings-plugin-ui/native: the components of ui.d.ts that pages drawn by the native Wings app can use, with the same
// names and props, so a page's JSX doesn't change. The page renders to a tree the app draws, not to a DOM: there are no
// `className`, `style` or `aria-*` props, and a handler gets `NativeEvent`, not a DOM event.
import type { ReactElement, ReactNode } from "react";

import type { ActionState, BadgeVariant, ButtonSize, CodeLine, ButtonVariant, Check, ComboboxOption, Gap, IconName, MenuItem, Option, Stat, Tone } from "./ui";

/** What a handler is given: the app's event, not a DOM one. The two methods do nothing, so code written for `ui` can call them. */
export interface NativeEvent {
  type: "click";
  preventDefault(): void;
  stopPropagation(): void;
}

/** A key pressed on a button, a row or a field. The app sends only the keys a page acts on: Enter and Escape in a field, Delete and Alt+Left or Alt+Right on a button or row. */
export interface NativeKeyEvent {
  type: "keydown";
  key: string;
  altKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  preventDefault(): void;
  stopPropagation(): void;
}

/**
 * Shows `element` as the page. One call per page; calling again renders the new element. Its hooks and handlers run in
 * the page's own JS engine. `fill` lays the page out edge to edge with no padding, for a page that scrolls on its own.
 */
export function mount(element: ReactElement, options?: { fill?: boolean }): void;

/** A page that fills a tab: a `toolbar`, usually a `Toolbar`, over a body that scrolls. `flush` takes the body's padding and scrolling away. */
export function Page(props: { toolbar?: ReactNode; flush?: boolean; children?: ReactNode }): ReactElement;
/** A page's top row: a `title` (cut to one line) on the left, `children` like filters after it, and `actions` on the right. */
export function Toolbar(props: { title?: ReactNode; actions?: ReactNode; children?: ReactNode }): ReactElement;
/** Children one under the other. With `maxWidth`, a column at most that many px wide, centred in the room it has. */
export function Stack(props: { gap?: Gap; grow?: boolean; maxWidth?: number; children?: ReactNode }): ReactElement;
/** Children side by side, centred. */
export function Row(props: { gap?: Gap; align?: "start" | "baseline"; justify?: "end" | "between"; wrap?: boolean; grow?: boolean; children?: ReactNode }): ReactElement;
/** Text with a tone and size, that can `grow` to fill a row or `truncate` with an ellipsis. */
export function Text(props: {
  tone?: Tone;
  /** small is 12px, tiny 11px. */
  size?: "small" | "tiny";
  weight?: "medium" | "strong";
  mono?: boolean;
  numeric?: boolean;
  truncate?: boolean;
  /** Up to three lines. */
  clamp?: boolean;
  grow?: boolean;
  /** Inside another `Text` it makes a link: the span is clicked like a button. */
  onClick?: (event: NativeEvent) => void;
  children?: ReactNode;
}): ReactElement;
/** A button. `variant` "default" is the filled one. `busy` shows a spinner and ignores clicks. An `aria-label` names a button that holds only an icon. */
export function Button(props: { variant?: ButtonVariant; size?: "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg"; onClick?: (event: NativeEvent) => void; onKeyDown?: (event: NativeKeyEvent) => void; disabled?: boolean; busy?: boolean; "aria-label"?: string; children?: ReactNode }): ReactElement;
/** A small quiet button. With `pressed` it's a toggle. */
export function Chip(props: { pressed?: boolean; onClick?: (event: NativeEvent) => void; disabled?: boolean; "aria-label"?: string; children?: ReactNode }): ReactElement;
/** A short message in a tinted box. */
export function Notice(props: { tone?: "info" | "success" | "warn" | "danger"; children?: ReactNode }): ReactElement;
/** A spinner. With a label it is announced by the app. */
export function Spinner(props: { size?: "large"; label?: string }): ReactElement;
/** A checkbox with its label. `onChange` gets its next checked state. */
export function Checkbox(props: { checked?: boolean; disabled?: boolean; onChange?: (checked: boolean) => void; children?: ReactNode }): ReactElement;
export interface ConfirmAction {
  id: string;
  label: ReactNode;
  onConfirm: () => unknown;
  variant?: "default" | "destructive" | "outline";
}
/** An in-page confirmation card. Failed actions leave the card open and show their error. */
export function Confirm(props: { title: ReactNode; actions: ConfirmAction[]; cancelLabel?: string; onCancel: () => void; children?: ReactNode }): ReactElement;
/** A badge, like a label on a command or a branch name. `tone` is Wings' own: a colour that says something. */
export function Badge(props: { variant?: BadgeVariant; tone?: Exclude<Tone, "muted">; size?: "sm"; children?: ReactNode }): ReactElement;
/** What shows before the first data comes: "Loading…", unless `children` say otherwise. */
export function Loading(props: { children?: ReactNode }): ReactElement;
/** Nothing to show: what's missing, a `description` on what to do about it, and `children` under them. */
export function Empty(props: { title: ReactNode; description?: ReactNode; children?: ReactNode }): ReactElement;
/** A box around related content. */
export function Card(props: { children?: ReactNode }): ReactElement;
/** A section heading, with `actions` like chips or a button on its right. */
export function Heading(props: { level?: 2 | 3 | 4; actions?: ReactNode; children?: ReactNode }): ReactElement;
/** A status dot, green unless `tone` says otherwise. `color` is a CSS `#rrggbb` or `rgb()` colour from your data, which wins over `tone`. */
export function Dot(props: { tone?: Exclude<Tone, "success">; color?: string }): ReactElement;
/** A labelled bar with its percentage and a `note` under it. `levels` are where it turns warn and danger, like `[75, 90]`. */
export function Gauge(props: { label: string; percent: number; note?: ReactNode; levels?: [number, number] }): ReactElement;
/** A list of rows. */
export function List(props: { children?: ReactNode }): ReactElement;
/** A row in a `List`. */
export function ListItem(props: { children?: ReactNode }): ReactElement;
/** More behind a `summary` line that opens it. The app keeps whether it's open, and it starts closed. */
/** A summary that folds `children` open and shut, shut at first; with `open` the page decides, and `onOpenChange` hears each click. */
export function Disclosure(props: { summary: ReactNode; open?: boolean; onOpenChange?: (open: boolean) => void; children?: ReactNode }): ReactElement;
/** A standalone progress bar, as wide as its parent. */
export function Meter(props: { label: string; percent: number; tone?: "success" | "warn" | "danger" }): ReactElement;
/** Columns side by side that scroll sideways, named `label`. */
export function Board(props: { label: string; children?: ReactNode }): ReactElement;
/** A column of a `Board`: its `title` and `count` over its cards, `children`. */
export function BoardColumn(props: { title: ReactNode; count?: number; actions?: ReactNode; children?: ReactNode }): ReactElement;
/** A board in rows: the `columns`' headers once, over `BoardLane`s. */
export function BoardLanes(props: { label: string; columns: { id: string; title: ReactNode; count?: number }[]; children?: ReactNode }): ReactElement;
/** A row of `BoardLanes` that folds: open at first unless `defaultOpen` is false, or as `open` says. Its `children` are `BoardCell`s. */
export function BoardLane(props: { label: string; title?: ReactNode; count?: number; actions?: ReactNode; open?: boolean; defaultOpen?: boolean; onOpenChange?: (open: boolean) => void; children?: ReactNode }): ReactElement;
/** One column's cards in a `BoardLane`. */
export function BoardCell(props: { label: string; children?: ReactNode }): ReactElement;
/** A select of `options`; without a `value` it starts on the first. `onChange` gets the value picked. */
export function Select<V extends string>(props: { label?: string; options: (Option<V> & { label: string; disabled?: boolean })[]; value?: V; onChange?: (value: V) => void }): ReactElement;

export function Chips<V extends string>(props: { label: string; options: { value: V; label: string; disabled?: boolean }[]; value: V; onChange: (value: V) => void }): ReactElement;
export function ListButton(props: { onClick?: (event: NativeEvent) => void; onKeyDown?: (event: NativeKeyEvent) => void; disabled?: boolean; align?: "baseline"; layout?: "column"; title?: string; indent?: number; "aria-expanded"?: boolean; children?: ReactNode }): ReactElement;
export function SearchField(props: { label: string; placeholder?: string; defaultValue?: string; onInput?: (event: NativeEvent & { currentTarget: { value: string }; target: { value: string } }) => void }): ReactElement;
export interface GraphRow {
  hash: string;
  subject: string;
  author: string;
  date: string;
  refs: string[];
  lane: number;
  color: number;
  merge: boolean;
  lines: number[];
}
/** A virtual list of at most 10,000 rows. At most 256 rows may be supplied in a window. */
export function Graph(props: { total: number; lanes: number; selected: number; offset: number; rows: GraphRow[]; onRangeChange: (range: { start: number; end: number }) => void; onSelect: (index: number) => void }): ReactElement;
/** A labelled field with a `hint` or an `error` under it, for the input in `children`, which takes the label as its name. */
export function Field(props: { label: ReactNode; hint?: ReactNode; error?: ReactNode; inline?: boolean; children?: ReactNode }): ReactElement;
/** Several controls under one label, with a `hint` or an `error` under them. */
export function FieldGroup(props: { label: ReactNode; hint?: ReactNode; error?: ReactNode; children?: ReactNode }): ReactElement;
/** What an input's `onChange` is given: the text is `target.value`. */
export interface NativeInputEvent {
  type: "change";
  preventDefault(): void;
  stopPropagation(): void;
  target: { value: string };
  currentTarget: { value: string };
}
/** A one-line text input. The page holds the text: give it `value` and set it in `onChange`. Outside a `Field`, give it a `label`. */
export function TextInput(props: { label?: string; /** `"password"` shows dots and can't be copied; any other type is plain text natively. */ type?: string; value?: string; placeholder?: string; disabled?: boolean; maxLength?: number; onChange?: (event: NativeInputEvent) => void; onKeyDown?: (event: NativeKeyEvent) => void; onBlur?: () => void; autoFocus?: boolean; selectOnFocus?: boolean }): ReactElement;
/** A multi-line text input, `rows` high at first. Outside a `Field`, give it a `label`. */
export function Textarea(props: { label?: string; value?: string; placeholder?: string; disabled?: boolean; maxLength?: number; rows?: number; onChange?: (event: NativeInputEvent) => void; onKeyDown?: (event: NativeKeyEvent) => void; onBlur?: () => void; autoFocus?: boolean; selectOnFocus?: boolean }): ReactElement;
/** Edits `value` in place: Enter or leaving the field saves it once with `onSave`, Escape cancels, and a failed save keeps the draft and shows why. `onDone` closes it. */
export function InlineInput(props: { label: string; value: string; required?: boolean; maxLength?: number; placeholder?: string; onSave: (value: string) => Promise<unknown>; onDone: () => void; onChange?: (event: NativeInputEvent) => void }): ReactElement;
/** A text field with the options that match it under it as chips; the app has no popup list yet. */
export function Combobox(props: {
  label?: string;
  value: string;
  onInput?: (text: string) => void;
  options: ComboboxOption[];
  onPick: (option: ComboboxOption) => void;
  freeText?: boolean;
  placeholder?: string;
  maxLength?: number;
  disabled?: boolean;
  /** Takes the keyboard when it first shows, and the page scrolls it into view. */
  autoFocus?: boolean;
  filter?: (option: ComboboxOption, query: string) => boolean;
  empty?: ReactNode;
}): ReactElement;
/** A button that shows its `items` under it when pressed; the app has no popup menu yet. */
export function Menu(props: { label: ReactNode; items: MenuItem[]; variant?: ButtonVariant; size?: ButtonSize; disabled?: boolean; "aria-label"?: string }): ReactElement;
/** One of Lucide's icons, in the colour of the text. */
export function Icon(props: { name: IconName; label?: string }): ReactElement;
/** How an `Svg` paints a shape: the app draws it in one colour, so a shape is painted in it or not at all. */
export type SvgPaint = "none" | "currentColor";
/**
 * A small drawing of your own, as `ui`'s: SVG shapes in a square `viewBox`, `size` px square (16 by default, up to 64),
 * in `tone`'s colour or the text's. The app draws `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`
 * and `g` with their geometry and the paint props below, on the svg or on a shape, in numbers without units, and
 * `opacity` makes a part fainter. Up to 32 shapes and 4 KB of markup; an `Svg` with any other element, prop or value
 * isn't drawn.
 */
export function Svg(props: {
  viewBox: string;
  size?: number;
  tone?: Tone;
  label?: string;
  fill?: SvgPaint;
  stroke?: SvgPaint;
  strokeWidth?: number | string;
  strokeLinecap?: "butt" | "round" | "square";
  strokeLinejoin?: "miter" | "round" | "bevel";
  fillRule?: "nonzero" | "evenodd";
  opacity?: number | string;
  children?: ReactNode;
}): ReactElement;
/** Small figures side by side, label over value. A `title` isn't shown. */
export function Stats(props: { items: Stat[] }): ReactElement;
/** Label and value pairs, one per line. */
export function Properties(props: { items: { label: string; value: ReactNode }[] }): ReactElement;
export { ago } from "wings-plugin-sdk/format";
/** The language to highlight a file as, from its path. */
export function languageOf(path: string | undefined | null): string | undefined;
/** Puts `text` on the clipboard, through the app, within 5 s of a click or key in the page (up to 1 MB), as a browser allows. */
export function copyText(text: string): Promise<void>;
/** A page's or a card's title: 14px, or 15px with `size="large"`. `truncate` cuts it to one line. */
export function Title(props: { size?: "large"; truncate?: boolean; children?: ReactNode }): ReactElement;
/** A round avatar with the initials of `name` (up to two letters), on a colour that depends only on the name. */
export function Avatar(props: { name: string; size?: "small" | "large" }): ReactElement;
/** A block that stands in for loading content, `width` and `height` in px; `width` may be a percentage like "40%". */
export function Skeleton(props: { width?: number | string; height?: number }): ReactElement;
/** A group of `Skeleton`s announced once as `label`. */
export function Skeletons(props: { label: string; children?: ReactNode }): ReactElement;
/** A row of `SummaryChip`s under a page's header. */
export function Summary(props: { children?: ReactNode }): ReactElement;
/** One fact in a `Summary`: a button when it has `onClick`, a badge otherwise. `tone` colours it. */
export function SummaryChip(props: { tone?: "info" | "success" | "warn" | "danger"; onClick?: (event: NativeEvent) => void; children?: ReactNode }): ReactElement;
/** A conversation or an activity feed. */
export function Timeline(props: { label?: string; children?: ReactNode }): ReactElement;
/** An entry of a `Timeline`: `author`'s avatar, name and time `at` (ISO), an optional `tag` and `actions`, then `children`. */
export function TimelineCard(props: { author: string; at?: string; tag?: ReactNode; actions?: ReactNode; children?: ReactNode }): ReactElement;
/** A quiet one-line entry of a `Timeline`: a small avatar, the author in bold, `children`, then the time. */
export function TimelineNote(props: { author: string; at?: string; children?: ReactNode }): ReactElement;
/** Tabs that pick one of `items` over the panel they show, `children`. They are drawn as `Chips`. */
export function Tabs<V extends string>(props: { label: string; items: { value: V; label: string; disabled?: boolean }[]; value: V; onChange: (value: V) => void; variant?: "default" | "line"; children?: ReactNode }): ReactElement;
/** A `list` pane and a `detail` pane side by side; `empty` shows in place of a missing `detail`. */
export function ListDetail(props: { list: ReactNode; detail?: ReactNode; empty?: ReactNode; listWidth?: number; listLabel?: string; detailLabel?: string; onBack?: () => void; backLabel?: string }): ReactElement;
/** Markdown as nodes: headings, paragraphs, lists, code, quotes, tables and folds, with bold, code and links inside lines. No HTML is parsed, and images show as their alt text. */
export function Markdown(props: { source: string; inline?: boolean }): ReactElement;
/** Code from `code` or `lines`, in a card, without colours by language. */
export type { CodeLine };
export function CodeBlock(props: { code?: string; lines?: CodeLine[]; lang?: string; title?: string; label?: string; busy?: boolean }): ReactElement;
/** How long ago `iso` was (after `prefix`), as text. Renders nothing when `iso` isn't a date. */
export function Time(props: { iso: string; prefix?: string; now?: number }): ReactElement | null;

/** The project the page is about: `undefined` until the app says, then the project, or `null` when none is open. Read once. */
export function useProject(): WingsProject | null | undefined;
/** What `wings.openView` opened the tab with, as `check` reads it, or `null` when there were no args or `check` returns `null` or `undefined` for them. Read once. */
export function useViewArgs<T>(check: Check<T>): T | null;
/** Shows `children(project)` once the app says which project the page is about, keyed by the project's id. */
export function ProjectGate(props: { loading?: ReactNode; empty?: { title?: ReactNode; description?: ReactNode }; children: (project: WingsProject) => ReactNode }): ReactElement;
/** The state the tab saved with `wings.setTabState`, as `check` reads it, or `initial`; `null` until it is read. The native app keeps it with the tab across restarts. */
export function useTabState<T>(initial: T, check: Check<T>): [T | null, (next: T) => void];
/** Like `useTabState`, for a block: the state saved with `wings.setBlockState`, kept with the Project page. */
export function useBlockState<T>(initial: T, check: Check<T>): [T | null, (next: T) => void];
/** Runs the page's async actions and keeps what its buttons and error card show: `busy`, the last `error`, `run` and `clear`. */
export function useAction(): ActionState;

/** A strip across its parent, like a page's header or footer: padded as a toolbar is, children one under the other, with a hairline on its `edge`. */
export function Bar(props: { edge?: "top" | "bottom"; gap?: Gap; children?: ReactNode }): ReactElement;
/** A column that scrolls when its content is taller than the room it has, and takes what is left of its parent. In the app it keeps 8 points of room either side and 12 below, as a list under a header has. */
export function Scroll(props: { children?: ReactNode }): ReactElement;
/** Two panes side by side: `start` is `defaultSize` points wide (280 by default) and `end` takes the rest. The app has no handle to drag yet. */
export function Split(props: { start: ReactNode; end: ReactNode; defaultSize?: number }): ReactElement;
/** One wedge of a `Sunburst`: from `from` to `to` radians clockwise from 12 o'clock, in `ring` (0 is the first round the centre). */
export interface SunburstWedge {
  from: number;
  to: number;
  ring: number;
  /** A CSS `#rrggbb`, `rgb()` or `rgba()` colour. */
  color: string;
  /** How opaque it is, 0 to 1, on top of the colour's own. */
  alpha?: number;
}
/**
 * Wedges in `rings` rings round a centre with a `title` and a `detail` in it, filling its pane. The page lays the wedges out, by ring
 * and in each ring by angle, at most 8,000 of them. `onPoint` gets the index of the wedge under the pointer, -1 for none and -2 for the
 * centre, when that changes; `onSelect` gets the index of a wedge or -2 on a click, and whether Cmd was held. `up` says a click on the centre does something.
 */
export function Sunburst(props: {
  rings: number;
  wedges: SunburstWedge[];
  selected?: number;
  title: string;
  detail?: string;
  up?: boolean;
  onPoint?: (index: number) => void;
  onSelect?: (index: number, event: { meta: boolean }) => void;
}): ReactElement;

/** Calls `load` when the component mounts and each time `load` changes, then every `ms` while the app is focused. */
export function usePoll(load: () => unknown, ms: number): void;
