import type { SVGProps } from "react";

/*
 * Marks for the agent CLIs without an official open-source mark we may ship. These are plain glyphs drawn for
 * Wings, not the companies' logos: they follow `currentColor` and only tell the panes apart.
 */

/** Codex: a prompt chevron and cursor. */
export function CodexIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round" {...props}>
      <path d="M5 7l6 5-6 5" />
      <path d="M13 18h6" />
    </svg>
  );
}

/** Cursor CLI: a pointer arrow. */
export function CursorIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" strokeWidth={1.6} strokeLinejoin="round" {...props}>
      <path d="M5 3l14 7-6 2-2 6z" />
    </svg>
  );
}

/** Grok Build: a ring around a dot. */
export function GrokIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.4} {...props}>
      <circle cx="12" cy="12" r="8" />
      <circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none" />
    </svg>
  );
}
