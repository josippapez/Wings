import { Children, isValidElement, type ComponentProps, type ReactElement, type ReactNode } from "react";
import { Card, Row, Stack, Text } from "wings-plugin-ui";

/** The native Stack, which also takes a fixed `width` in px (wings-plugin-ui/native.js). */
export const Column = Stack as unknown as (props: ComponentProps<typeof Stack> & { width?: number }) => ReactElement;

/** The classes of tasks.css that lay their children side by side. */
const ROWS = ["tk-crumbs", "tk-moves", "tk-col-title", "tk-lane-title", "tk-link", "tk-file-path", "tk-file-counts"];
/** The classes of tasks.css that draw a span muted. */
const MUTED = ["tk-crumb-sep", "tk-crumb", "tk-count", "tk-lane-none", "tk-sha", "tk-tree-elbow", "tk-column-empty", "tk-view-hint"];

export function Layout({ tag, className = "", children }: { tag: string; className?: string; children?: ReactNode }) {
  const classes = className.split(" ");
  const is = (names: string[]) => names.some((c) => classes.includes(c));
  // A row's time and status, muted and as wide as tasks.css's min-widths, so they line up down the list.
  if (classes.includes("tk-row-time")) {
    // The empty one in a group's head: the head's card takes 13px more at its right end than a row does.
    if (!children) return <Column width={43} />;
    return (
      <Column width={56}>
        <Row justify="end"><Text tone="muted" size="tiny">{children}</Text></Row>
      </Column>
    );
  }
  // A row's parent, in the list by status: its icon and its title, muted and small, cut short to leave the row's own.
  if (classes.includes("tk-row-parent")) {
    const [icon, ...title] = Children.toArray(children);
    return <Row gap="s">{icon}<Text tone="muted" size="tiny" truncate>{title}</Text></Row>;
  }
  if (classes.includes("tk-row-status")) {
    const [dot, ...label] = Children.toArray(children);
    return (
      <Column width={92}>
        <Row gap="s">{dot}<Text tone="muted" size="tiny">{label}</Text></Row>
      </Column>
    );
  }
  // A group's head in the list: tasks.css's bar on the surface colour.
  if (classes.includes("tk-tree-head")) return <Card><Row gap="s">{children}</Row></Card>;
  if (classes.includes("tk-key")) return <Text tone="muted" size="tiny" mono>{children}</Text>;
  if (classes.includes("tk-card-foot")) return <Row gap="s" justify="between">{children}</Row>;
  // The description with its Edit button at the top right, as tasks.css lays it out; in a column the button took the
  // whole width and its word sat in the middle of the page.
  if (classes.includes("tk-description")) return <Row gap="s" align="start" justify="between">{children}</Row>;
  // tasks.css's grid: the title and the description in a column, and the properties in a 240px one beside it.
  if (classes.includes("tk-grid")) {
    const [head, props, ...main] = Children.toArray(children);
    return (
      <Row gap="l" align="start">
        <Stack gap="l" grow>{head}{main}</Stack>
        {/* With the row's gaps, the grid's 40px between the columns. */}
        <Column width={16} />
        {props}
      </Row>
    );
  }
  if (classes.includes("tk-props")) return <Column gap="l" width={240}>{children}</Column>;
  // tasks.css's title row: the type icon centred on the title, and the key right after the title on its baseline. The
  // title's button has its own padding round the text, so it goes right after the icon and the key 8px after it: the
  // text sits 11px from the icon and 19px from the key, as the desktop's button pulled 8px over the row's gap puts it.
  if (classes.includes("tk-title-row")) {
    const [icon, title, key] = Children.toArray(children);
    const button = isValidElement<{ wrap?: boolean }>(title) && title.props.wrap === true;
    return (
      <Row gap={button ? "none" : "m"}>
        {icon}
        <Row gap={button ? "m" : "l"} align="baseline" grow>{title}{key}</Row>
      </Row>
    );
  }
  if (is(ROWS)) return <Row gap="s">{children}</Row>;
  if (is(MUTED)) return <Text tone="muted" size="small">{children}</Text>;
  if (tag === "span") return <Text grow={classes.includes("tk-link-title")} truncate={classes.includes("tk-link-title")}>{children}</Text>;
  if (tag === "p") return <Text>{children}</Text>;
  // tasks.css's centred columns: the issue page's and the list's.
  const maxWidth = classes.includes("tk-wrap") ? 1040 : classes.includes("tk-list") ? 1100 : undefined;
  return <Stack gap={classes.includes("tk-wrap") || classes.includes("tk-main") || classes.includes("tk-grid") || classes.includes("tk-list") ? "l" : "s"} maxWidth={maxWidth}>{children}</Stack>;
}
