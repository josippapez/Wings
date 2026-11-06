// wings-plugin-ui/native: the components of ui.d.ts that pages drawn by the native Wings app can use, with the same
// names and props, so a page's JSX doesn't change. The page renders to a tree the app draws, not to a DOM: there are no
// `className`, `style` or `aria-*` props, and a handler gets `NativeEvent`, not a DOM event.
import type { ReactElement, ReactNode } from "react";

import type { ActionState, BadgeVariant, ButtonVariant, Check, ComboboxOption, Gap, IconName, MenuItem, Option, Tone } from "./ui";

/** What a handler is given: the app's event, not a DOM one. The two methods do nothing, so code written for `ui` can call them. */
export interface NativeEvent {
  type: "click";
  preventDefault(): void;
  stopPropagation(): void;
}

/** Shows `element` as the page. One call per page; calling again renders the new element. Its hooks and handlers run in the page's own JS engine. */
export function mount(element: ReactElement): void;

/** A page that fills a tab: a `toolbar`, usually a `Toolbar`, over a body that scrolls. `flush` takes the body's padding and scrolling away. */
export function Page(props: { toolbar?: ReactNode; flush?: boolean; children?: ReactNode }): ReactElement;
/** A page's top row: a `title` (cut to one line) on the left, `children` like filters after it, and `actions` on the right. */
export function Toolbar(props: { title?: ReactNode; actions?: ReactNode; children?: ReactNode }): ReactElement;
/** Children one under the other. */
export function Stack(props: { gap?: Gap; grow?: boolean; children?: ReactNode }): ReactElement;
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
  children?: ReactNode;
}): ReactElement;
/** A button. `variant` "default" is the filled one. `busy` shows a spinner and ignores clicks. An `aria-label` names a button that holds only an icon. */
export function Button(props: { variant?: ButtonVariant; onClick?: (event: NativeEvent) => void; disabled?: boolean; busy?: boolean; "aria-label"?: string; children?: ReactNode }): ReactElement;
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
/** A section heading. */
export function Heading(props: { children?: ReactNode }): ReactElement;
/** A status dot, green unless `tone` says otherwise. */
export function Dot(props: { tone?: Exclude<Tone, "success"> }): ReactElement;
/** A labelled bar with its percentage and a `note` under it. `levels` are where it turns warn and danger, like `[75, 90]`. */
export function Gauge(props: { label: string; percent: number; note?: ReactNode; levels?: [number, number] }): ReactElement;
/** A list of rows. */
export function List(props: { children?: ReactNode }): ReactElement;
/** A row in a `List`. */
export function ListItem(props: { children?: ReactNode }): ReactElement;
/** More behind a `summary` line that opens it. The app keeps whether it's open, and it starts closed. */
export function Disclosure(props: { summary: ReactNode; children?: ReactNode }): ReactElement;
/** A select of `options`; without a `value` it starts on the first. `onChange` gets the value picked. */
export function Select<V extends string>(props: { label?: string; options: (Option<V> & { label: string; disabled?: boolean })[]; value?: V; onChange?: (value: V) => void }): ReactElement;

export function Chips<V extends string>(props: { label: string; options: { value: V; label: string; disabled?: boolean }[]; value: V; onChange: (value: V) => void }): ReactElement;
export function ListButton(props: { onClick?: (event: NativeEvent) => void; disabled?: boolean; align?: "baseline"; layout?: "column"; title?: string; indent?: number; "aria-expanded"?: boolean; children?: ReactNode }): ReactElement;
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
export function TextInput(props: { label?: string; value?: string; placeholder?: string; disabled?: boolean; maxLength?: number; onChange?: (event: NativeInputEvent) => void }): ReactElement;
/** A multi-line text input, `rows` high at first. Outside a `Field`, give it a `label`. */
export function Textarea(props: { label?: string; value?: string; placeholder?: string; disabled?: boolean; maxLength?: number; rows?: number; onChange?: (event: NativeInputEvent) => void }): ReactElement;
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
  filter?: (option: ComboboxOption, query: string) => boolean;
  empty?: ReactNode;
}): ReactElement;
/** A button that shows its `items` under it when pressed; the app has no popup menu yet. */
export function Menu(props: { label: ReactNode; items: MenuItem[]; variant?: ButtonVariant; disabled?: boolean }): ReactElement;
/** One of Lucide's icons, in the colour of the text. */
export function Icon(props: { name: IconName; label?: string }): ReactElement;
/** Label and value pairs, one per line. */
export function Properties(props: { items: { label: string; value: ReactNode }[] }): ReactElement;
/** How long ago `iso` was (after `prefix`), as text. Renders nothing when `iso` isn't a date. */
export function Time(props: { iso: string; prefix?: string; now?: number }): ReactElement | null;

/** The project the page is about: `undefined` until the app says, then the project, or `null` when none is open. Read once. */
export function useProject(): WingsProject | null | undefined;
/** What the tab was opened with. The app opens a native page without arguments, so this is `null`. */
export function useViewArgs<T>(check: Check<T>): T | null;
/** Runs the page's async actions and keeps what its buttons and error card show: `busy`, the last `error`, `run` and `clear`. */
export function useAction(): ActionState;
