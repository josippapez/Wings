// Checked by `tsc` in `pnpm test`, never run: native.d.ts takes the props ui.d.ts does, so a page's JSX doesn't change.
import {
  Badge as NativeBadge,
  Button as NativeButton,
  Card as NativeCard,
  Disclosure as NativeDisclosure,
  Dot as NativeDot,
  Empty as NativeEmpty,
  Gauge as NativeGauge,
  Heading as NativeHeading,
  List as NativeList,
  ListItem as NativeListItem,
  Loading as NativeLoading,
  Page as NativePage,
  Row as NativeRow,
  Select as NativeSelect,
  Stack as NativeStack,
  Text as NativeText,
  Toolbar as NativeToolbar,
} from "./native";
import type { Badge, Button, Card, Disclosure, Dot, Empty, Gauge, Heading, List, ListItem, Loading, Page, Row, Select, Stack, Text, Toolbar } from "./ui";

// What a native component takes, a ui component takes too.
type Props<F> = F extends (props: infer P) => unknown ? P : never;
type Takes<Native, Web> = Props<Native> extends Props<Web> ? true : never;
export const sameProps: [
  Takes<typeof NativeBadge, typeof Badge>,
  Takes<typeof NativeCard, typeof Card>,
  Takes<typeof NativeDisclosure, typeof Disclosure>,
  Takes<typeof NativeDot, typeof Dot>,
  Takes<typeof NativeEmpty, typeof Empty>,
  Takes<typeof NativeGauge, typeof Gauge>,
  Takes<typeof NativeHeading, typeof Heading>,
  Takes<typeof NativeList, typeof List>,
  Takes<typeof NativeListItem, typeof ListItem>,
  Takes<typeof NativeLoading, typeof Loading>,
  Takes<typeof NativePage, typeof Page>,
  Takes<typeof NativeRow, typeof Row>,
  Takes<typeof NativeSelect, typeof Select>,
  Takes<typeof NativeStack, typeof Stack>,
  Takes<typeof NativeText, typeof Text>,
  Takes<typeof NativeToolbar, typeof Toolbar>,
  // A button's handler gets the native event, so only its other props are the same.
  Props<typeof NativeButton> extends Omit<Props<typeof Button>, "onClick"> ? true : never,
] = [true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true, true];

export const page = (
  <NativePage toolbar={<NativeToolbar title="Pilot" actions={<NativeBadge tone="info">ready</NativeBadge>} />}>
    <NativeButton variant="outline" onClick={(event) => event.stopPropagation()} disabled>
      Add
    </NativeButton>
    <NativeButton onClick={() => {}}>No event needed</NativeButton>
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
  // @ts-expect-error a select's handler gets the value picked, not an event
  <NativeSelect options={[{ value: "a", label: "A" }]} onChange={(event: MouseEvent) => event.clientX} />,
];
