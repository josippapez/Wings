// Checked by `tsc` in `pnpm test`, never run: native.d.ts takes the props ui.d.ts does, so a page's JSX doesn't change.
import {
  Badge as NativeBadge,
  Button as NativeButton,
  Card as NativeCard,
  Checkbox as NativeCheckbox,
  Chip as NativeChip,
  Chips as NativeChips,
  Combobox as NativeCombobox,
  Confirm as NativeConfirm,
  Disclosure as NativeDisclosure,
  Dot as NativeDot,
  Empty as NativeEmpty,
  Field as NativeField,
  FieldGroup as NativeFieldGroup,
  Gauge as NativeGauge,
  Graph as NativeGraph,
  Heading as NativeHeading,
  Icon as NativeIcon,
  List as NativeList,
  ListItem as NativeListItem,
  ListButton as NativeListButton,
  Loading as NativeLoading,
  Menu as NativeMenu,
  Notice as NativeNotice,
  Page as NativePage,
  Properties as NativeProperties,
  Row as NativeRow,
  Select as NativeSelect,
  SearchField as NativeSearchField,
  Stack as NativeStack,
  Spinner as NativeSpinner,
  Svg as NativeSvg,
  Text as NativeText,
  TextInput as NativeTextInput,
  Textarea as NativeTextarea,
  Time as NativeTime,
  Toolbar as NativeToolbar,
} from "./native";
import type { Badge, Button, Card, Checkbox, Chip, Combobox, Confirm, Disclosure, Dot, Empty, Field, FieldGroup, Gauge, Heading, Icon, List, ListItem, Loading, Menu, Notice, Page, Properties, Row, Select, Spinner, Stack, Svg, Text, TextInput, Textarea, Time, Toolbar } from "./ui";

// What a native component takes, a ui component takes too.
type Props<F> = F extends (props: infer P) => unknown ? P : never;
type Takes<Native, Web> = Props<Native> extends Props<Web> ? true : never;
export const sameProps: [
  Takes<typeof NativeBadge, typeof Badge>,
  Takes<typeof NativeCard, typeof Card>,
  Takes<typeof NativeCheckbox, typeof Checkbox>,
  Takes<typeof NativeConfirm, typeof Confirm>,
  Takes<typeof NativeDisclosure, typeof Disclosure>,
  Takes<typeof NativeDot, typeof Dot>,
  Takes<typeof NativeEmpty, typeof Empty>,
  Takes<typeof NativeGauge, typeof Gauge>,
  Takes<typeof NativeHeading, typeof Heading>,
  Takes<typeof NativeList, typeof List>,
  Takes<typeof NativeListItem, typeof ListItem>,
  Takes<typeof NativeLoading, typeof Loading>,
  Takes<typeof NativeNotice, typeof Notice>,
  Takes<typeof NativePage, typeof Page>,
  Takes<typeof NativeRow, typeof Row>,
  Takes<typeof NativeSelect, typeof Select>,
  Takes<typeof NativeStack, typeof Stack>,
  Takes<typeof NativeSpinner, typeof Spinner>,
  Takes<typeof NativeToolbar, typeof Toolbar>,
  Takes<typeof NativeCombobox, typeof Combobox>,
  Takes<typeof NativeField, typeof Field>,
  Takes<typeof NativeFieldGroup, typeof FieldGroup>,
  Takes<typeof NativeIcon, typeof Icon>,
  Takes<typeof NativeMenu, typeof Menu>,
  Takes<typeof NativeProperties, typeof Properties>,
  Takes<typeof NativeTime, typeof Time>,
  Takes<typeof NativeSvg, typeof Svg>,
  // A handler gets the native event, so only the other props are the same.
  Props<typeof NativeButton> extends Omit<Props<typeof Button>, "onClick" | "onKeyDown"> ? true : never,
  Props<typeof NativeChip> extends Omit<Props<typeof Chip>, "onClick"> ? true : never,
  Props<typeof NativeText> extends Omit<Props<typeof Text>, "onClick"> ? true : never,
  Props<typeof NativeTextInput> extends Omit<Props<typeof TextInput>, "onChange" | "onKeyDown" | "onBlur"> ? true : never,
  Props<typeof NativeTextarea> extends Omit<Props<typeof Textarea>, "onChange" | "onKeyDown" | "onBlur"> ? true : never,
] = [true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true];

export const page = (
  <NativePage toolbar={<NativeToolbar title="Pilot" actions={<NativeBadge tone="info">ready</NativeBadge>} />}>
    <NativeButton variant="outline" onClick={(event) => event.stopPropagation()} disabled>
      Add
    </NativeButton>
    <NativeButton onClick={() => {}}>No event needed</NativeButton>
    <NativeCheckbox checked onChange={() => {}}>Delete branch</NativeCheckbox>
    <NativeNotice tone="warn">Careful</NativeNotice>
    <NativeSpinner label="Loading" />
    <NativeChips label="Show" options={[{ value: "history", label: "History" }]} value="history" onChange={(value) => value.toUpperCase()} />
    <NativeSearchField label="Search commits" defaultValue="coupon" onInput={(event) => event.currentTarget.value.toUpperCase()} />
    <NativeListButton layout="column" indent={12} onClick={(event) => event.stopPropagation()}>Open commit</NativeListButton>
    <NativeGraph total={10000} lanes={2} offset={0} rows={[]} selected={-1} onRangeChange={({ start, end }) => end - start} onSelect={(index) => index.toFixed()} />
    <NativeConfirm title="Remove?" actions={[{ id: "remove", label: "Remove", onConfirm: () => {} }]} onCancel={() => {}} />
    <NativeCard>
      <NativeHeading>Usage</NativeHeading>
      <NativeGauge label="Memory" percent={42} note="5 of 12 GB" levels={[75, 90]} />
    </NativeCard>
    <NativeList>
      <NativeListItem>
        <NativeDot tone="danger" />
      </NativeListItem>
    </NativeList>
    <NativeDisclosure summary="More">inside</NativeDisclosure>
    <NativeSelect label="Session" options={[{ value: "a", label: "A" }]} value="a" onChange={(value) => value.toUpperCase()} />
  </NativePage>
);

export const notNative = [
  // @ts-expect-error there is no className: the app draws the page
  <NativeStack className="x" />,
  // @ts-expect-error a tone is one of ui's
  <NativeText tone="loud" />,
  // @ts-expect-error the handler gets a NativeEvent, not a DOM event
  <NativeButton onClick={(event: MouseEvent) => event.clientX} />,
  // @ts-expect-error a badge's tone leaves out "muted", like ui's
  <NativeBadge tone="muted" />,
  // @ts-expect-error a dot's tone leaves out "success", like ui's
  <NativeDot tone="success" />,
  // @ts-expect-error the app draws an svg in one colour: a shape is painted in it or not at all
  <NativeSvg viewBox="0 0 16 16" fill="red" />,
  // @ts-expect-error an svg needs its viewBox
  <NativeSvg size={14} />,
  // @ts-expect-error a select's handler gets the value picked, not an event
  <NativeSelect options={[{ value: "a", label: "A" }]} onChange={(event: MouseEvent) => event.clientX} />,
];

import { Stats as NativeStats } from "./native";

export const figures = (
  <NativeStack>
    <NativeStats items={[{ label: "Status", value: "Working", tone: "warn" }]} />
    <NativeHeading actions={<NativeButton>Filter</NativeButton>}>Recent activity</NativeHeading>
  </NativeStack>
);
