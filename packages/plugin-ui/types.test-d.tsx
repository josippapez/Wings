// Checked by `tsc` in `pnpm test`, never run: `as` types a layout component's props for the element it renders.
import type { FormEvent, MouseEvent } from "react";

import { Badge, Board, BoardColumn, Button, Card, Combobox, Empty, ListDetail, Menu, Page, Row, Select, Split, Stack, Tabs, Toolbar } from "./ui";

const submit = (e: FormEvent<HTMLFormElement>) => e.currentTarget.requestSubmit();

export const forms = [
  <Card as="form" onSubmit={(e) => e.currentTarget.requestSubmit()} compact />,
  <Stack as="form" onSubmit={submit} gap="l" />,
  <Row as="form" onSubmit={submit} gap="s" wrap />,
  // @ts-expect-error onSubmit isn't a `ul` prop
  <Stack as="ul" onSubmit={submit} />,
  // @ts-expect-error a `div` is not a `form`
  <Card onSubmit={submit} />,
  // @ts-expect-error the handler's element follows `as`
  <Row as="form" onClick={(e: MouseEvent<HTMLDivElement>) => e.currentTarget} />,
  // @ts-expect-error `section` is not one of the elements a Row can be
  <Row as="table" />,
];

// Without `as` they're still divs.
export const divs = [<Card onClick={(e) => e.currentTarget.offsetWidth} />, <Stack gap="m" />, <Row justify="between" />];

// Combobox's key handler and Enter hook.
export const combobox = (
  <Combobox
    value=""
    options={[]}
    onPick={() => {}}
    onKeyDown={(e) => e.currentTarget.value}
    onEnter={(text) => text.trim()}
  />
);

export const comboboxOpen = [
  <Combobox value="" options={[]} onPick={() => {}} open="whileText" />,
  // @ts-expect-error only "auto" and "whileText"
  <Combobox value="" options={[]} onPick={() => {}} open="always" />,
];

// shadcn/ui's names, and not the ones of 0.2 to 0.5.
export const buttons = [
  <Button variant="default" size="xs" />,
  <Button variant="secondary" size="sm" />,
  <Button variant="outline" size="lg" />,
  <Button variant="ghost" size="icon" />,
  <Button variant="destructive" size="icon-xs" />,
  <Button variant="link" busy />,
  // @ts-expect-error "primary" is "default"
  <Button variant="primary" />,
  // @ts-expect-error "danger" is "destructive"
  <Button variant="danger" />,
  // @ts-expect-error "small" is "sm"
  <Button size="small" />,
];

export const badges = [
  <Badge variant="secondary" tone="warn" />,
  // @ts-expect-error not a variant
  <Badge variant="pill" />,
];

export const others = [
  <Empty title="None" description="Try again" />,
  // @ts-expect-error "detail" is "description"
  <Empty title="None" detail="Try again" />,
  <Menu label="More" items={[{ id: "a", label: "Delete", variant: "destructive", onSelect() {} }]} variant="outline" size="sm" />,
  // @ts-expect-error "tone" is "variant"
  <Menu label="More" items={[{ id: "a", label: "Delete", tone: "danger", onSelect() {} }]} />,
  <Tabs label="Views" items={[{ value: "a", label: "A" }]} value="a" onChange={(v) => v.toUpperCase()} variant="line" />,
  <Select label="Pick" options={[{ value: "a", label: "A" }]} onChange={(v) => v.toUpperCase()} onFocus={(e) => e.currentTarget.click()} />,
];

// Page layouts.
export const pages = [
  <Page toolbar={<Toolbar title="Tasks" actions={<Button />} />} flush>
    <ListDetail list={<Stack />} detail={<Card />} listWidth={240} onBack={() => {}} />
    <Split start={<Stack />} end={<Stack />} direction="vertical" defaultSize={200} min={80} max={400} onResize={(size: number) => size} />
    <Board label="Board">
      <BoardColumn title="Todo" count={3} actions={<Button />} />
    </Board>
  </Page>,
  // @ts-expect-error a Split needs both panes
  <Split start={<Stack />} />,
  // @ts-expect-error a Board needs a label
  <Board />,
  // @ts-expect-error direction is horizontal or vertical
  <Split start={<Stack />} end={<Stack />} direction="diagonal" />,
];
