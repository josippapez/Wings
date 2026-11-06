// wings-plugin-ui/native: the components of ui.d.ts that pages drawn by the native Wings app can use, with the same
// names and props, so a page's JSX doesn't change. The page renders to a tree the app draws, not to a DOM: there are no
// `className`, `style` or `aria-*` props, and a handler gets `NativeEvent`, not a DOM event.
import type { ReactElement, ReactNode } from "react";

import type { BadgeVariant, ButtonVariant, Gap, Option, Tone } from "./ui";

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
/** A button. `variant` "default" is the filled one. */
export function Button(props: { variant?: ButtonVariant; onClick?: (event: NativeEvent) => void; disabled?: boolean; children?: ReactNode }): ReactElement;
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
