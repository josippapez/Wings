import { Children, type ComponentProps, type ReactElement, type ReactNode } from "react";
import { Row, Stack, Text } from "wings-plugin-ui";

/** The native Stack, which also takes a fixed `width` in px (wings-plugin-ui/native.js). */
const Column = Stack as unknown as (props: ComponentProps<typeof Stack> & { width?: number }) => ReactElement;

/** The classes of tasks.css that lay their children side by side. */
const ROWS = ["tk-crumbs", "tk-moves", "tk-col-title", "tk-lane-title", "tk-tree-head", "tk-row-status", "tk-row-parent", "tk-link", "tk-file-path", "tk-file-counts"];
/** The classes of tasks.css that draw a span muted. */
const MUTED = ["tk-key", "tk-crumb-sep", "tk-crumb", "tk-count", "tk-lane-none", "tk-sha", "tk-tree-elbow", "tk-column-empty", "tk-view-hint"];

export function Layout({ tag, className = "", children }: { tag: string; className?: string; children?: ReactNode }) {
  const classes = className.split(" ");
  const is = (names: string[]) => names.some((c) => classes.includes(c));
  if (classes.includes("tk-row-time")) return null;
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
  if (classes.includes("tk-title-row")) return <Row gap="m" align="start">{children}</Row>;
  if (is(ROWS)) return <Row gap="s">{children}</Row>;
  if (is(MUTED)) return <Text tone="muted" size="small">{children}</Text>;
  if (tag === "span") return <Text grow={classes.includes("tk-link-title")} truncate={classes.includes("tk-link-title")}>{children}</Text>;
  if (tag === "p") return <Text>{children}</Text>;
  // tasks.css's centred columns: the issue page's and the list's.
  const maxWidth = classes.includes("tk-wrap") ? 1040 : classes.includes("tk-list") ? 1100 : undefined;
  return <Stack gap={classes.includes("tk-wrap") || classes.includes("tk-main") || classes.includes("tk-grid") ? "l" : "s"} maxWidth={maxWidth}>{children}</Stack>;
}
