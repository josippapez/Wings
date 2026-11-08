// Small pieces the PR pages share: a check's mark, a tag, and what the native app needs told that pr.css says on the
// desktop.

import type { ComponentProps, ReactElement, ReactNode } from "react";
import { Badge, Stack, Svg, Text } from "wings-plugin-ui";
import type { CheckState } from "../lib/types";

const STATE_WORD: Record<CheckState, string> = { failed: "Failed", running: "Running", queued: "Queued", passed: "Passed", skipped: "Not run" };
const STATE_MARK: Record<CheckState, string> = { failed: "✕", running: "●", queued: "○", passed: "✓", skipped: "–" };
const STATE_TONE = { failed: "danger", running: "warn", queued: "muted", passed: "success", skipped: "muted" } as const;

/** The desktop's marks as drawings, as large as pr.css's bold glyphs in their 16px box. */
const STATE_SHAPE: Record<CheckState, ReactNode> = {
  failed: <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" />,
  running: <circle cx="8" cy="8" r="5.5" fill="currentColor" stroke="none" />,
  queued: <circle cx="8" cy="8" r="4.8" />,
  passed: <path d="M3.5 8.5l3 3 6-7" />,
  skipped: <path d="M4.5 8h7" />,
};

/**
 * How a check, job or step stands: ✕, ●, ○, ✓ or –, in its colour. The native app draws a glyph in the text's own
 * weight and size, a thin ring and cross, so there it's the same marks drawn. Their 16px box is 19 high there, a line
 * of 13px text, so a mark at the top of a row is centred on its first line as pr.css's 20px line-height centres it.
 */
export function StateMark({ state }: { state: CheckState }) {
  if (WINGS_NATIVE) {
    return (
      <Svg viewBox="-1.5 -1.5 19 19" size={19} tone={STATE_TONE[state]} label={STATE_WORD[state]} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
        {STATE_SHAPE[state]}
      </Svg>
    );
  }
  return <Text tone={STATE_TONE[state]} className="pr-check-mark" data-state={state} role="img" aria-label={STATE_WORD[state]}>{STATE_MARK[state]}</Text>;
}

/** A small outlined label, muted unless it's `required`, which is in the warning colour: pr.css's `.pr-tag`. */
export function Tag({ required, children }: { required?: boolean; children: ReactNode }) {
  if (WINGS_NATIVE) return <Badge size="sm" variant="outline" tone={required ? "warn" : undefined}>{required ? children : <Text tone="muted">{children}</Text>}</Badge>;
  return <Badge size="sm" variant="outline" className="pr-tag" data-tone={required ? "required" : undefined}>{children}</Badge>;
}

/** The native Stack, which also takes a fixed `width` in px (wings-plugin-ui/native.js): a column of a row. */
export const Column = Stack as unknown as (props: ComponentProps<typeof Stack> & { width?: number }) => ReactElement;

/** The native Text at its large size, 20px, as pr.css sizes a page's title. */
export const LargeText = Text as unknown as (props: Omit<ComponentProps<typeof Text>, "size"> & { size: "large" }) => ReactElement;
