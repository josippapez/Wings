import type { HTMLAttributes } from "react";
import type { LinkButtonProps } from "./layout";
import { Card, ListButton, Row, Stack, Text, Title } from "wings-plugin-ui";

/** The classes of item.css and sprint.css that lay their children side by side. */
const ROWS = ["wi-kicker", "wi-meta", "wi-card-head", "wi-rev-head", "wi-tags", "wi-person", "wi-change", "sprint-assignee"];
/** The classes of item.css that draw a span muted. */
const MUTED = ["wi-link-id", "wi-change-label", "wi-change-from", "wi-arrow"];

export function Layout({ tag, className = "", children }: HTMLAttributes<HTMLElement> & { tag: string }) {
  const classes = className.split(" ");
  if (classes.includes("wi-card")) return <Card><Stack gap="m">{children}</Stack></Card>;
  if (classes.includes("wi-rev")) return <Row gap="s" align="start">{children}</Row>;
  if (ROWS.some((c) => classes.includes(c))) return <Row gap="s" wrap>{children}</Row>;
  if (tag === "strong" || tag === "h3") return <Text weight="strong">{children}</Text>;
  if (tag === "em") return <Text tone="muted">{children}</Text>;
  // item.css puts a margin before the number beside the title.
  if (classes.includes("wi-number")) return <Text tone="muted"> {children}</Text>;
  if (tag === "span") return MUTED.some((c) => classes.includes(c)) ? <Text tone="muted">{children}</Text> : <Text grow={classes.includes("wi-link-title")}>{children}</Text>;
  if (tag === "h1") return <Title size="large">{children}</Title>;
  return <Stack gap={classes.includes("wi-wrap") || classes.includes("wi-overview") ? "l" : "s"} grow={classes.includes("wi-rev-body")}>{children}</Stack>;
}

export function LinkButton({ onClick, disabled, children, ...props }: LinkButtonProps) {
  return <ListButton disabled={disabled} title={props["aria-label"]} onClick={onClick}>{children}</ListButton>;
}
