// A work item type's icon: Azure DevOps' colour behind a simple glyph, so a list of mixed items reads at a glance.

import "./type-icon.css";
import { typeStyle, type Glyph } from "../lib/type";

const PATHS: Record<Glyph, string> = {
  bug: "M8 5.5c1.7 0 2.8 1.4 2.8 3.5S9.7 13 8 13 5.2 11.1 5.2 9 6.3 5.5 8 5.5zM6.2 6 4.5 4.5M9.8 6l1.7-1.5M5.2 9H3M10.8 9H13M5.6 11.6 4 13M10.4 11.6 12 13",
  task: "M4.5 8.5 7 11l4.5-5.5",
  story: "M5 3h6v10l-3-2.4L5 13z",
  feature: "M5 3h6v3a3 3 0 0 1-6 0zM8 9v3M6 13h4",
  epic: "M9 2.5 4.5 9H8l-1 4.5L11.5 7H8z",
  issue: "M8 4v5M8 11.6v.4",
  test: "M6.5 3h3M7 3v4l-3 5.5h8L9 7V3",
  other: "M8 6.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z",
};

/** The icon of a work item `type`, decoration beside its name. */
export function TypeIcon({ type, size = 18 }: { type: string; size?: number }) {
  const { glyph, color } = typeStyle(type);
  return (
    <span className="wi-type-icon" style={{ width: size, height: size, background: color, color: glyph === "task" ? "#2a2200" : "#fff" }} title={type} aria-hidden="true">
      <svg viewBox="0 0 16 16" width={size - 4} height={size - 4} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d={PATHS[glyph]} />
      </svg>
    </span>
  );
}
