// The Issues page in Wings Settings: which view the Issues tab opens in, picked from a small drawing of each.

import type { ReactNode } from "react";
import { useSetting } from "wings-plugin-sdk/react";
import { Chips, Heading, Stack, Text } from "wings-plugin-ui";
import { DEFAULT_VIEWS, defaultView, type DefaultView } from "../lib/view-setting";
import "./tasks.css";

export const page = { place: "settings" };

const COLS = [6, 34, 62, 90];

/** Status columns of cards. */
const board = (
  <>
    {COLS.map((x, i) => (
      <g key={x}>
        <rect x={x} y={6} width={24} height={4} rx={2} className="tk-view-head" />
        {Array.from({ length: [3, 2, 2, 1][i] }, (_, n) => (
          <rect key={n} x={x} y={14 + n * 14} width={24} height={10} rx={2} className="tk-view-card" />
        ))}
      </g>
    ))}
  </>
);

/** The same columns, with a band per epic or feature running across them. */
const grouped = (
  <>
    {COLS.map((x) => (
      <rect key={x} x={x} y={6} width={24} height={4} rx={2} className="tk-view-head" />
    ))}
    {[14, 40].map((y, band) => (
      <g key={y}>
        <rect x={6} y={y} width={108} height={22} rx={3} className="tk-view-band" />
        {COLS.filter((_, i) => (band ? i !== 1 : i !== 3)).map((x) => (
          <rect key={x} x={x + 2} y={y + 6} width={20} height={10} rx={2} className="tk-view-card" />
        ))}
      </g>
    ))}
  </>
);

/** One line per issue, under status headings. */
const list = (
  <>
    {[6, 36].map((y, s) => (
      <g key={y}>
        <rect x={6} y={y} width={30} height={4} rx={2} className="tk-view-head" />
        {Array.from({ length: s ? 2 : 3 }, (_, n) => (
          <rect key={n} x={6} y={y + 8 + n * 7} width={108} height={5} rx={1.5} className="tk-view-card" />
        ))}
      </g>
    ))}
  </>
);

const PICTURES: Record<DefaultView, { drawing: ReactNode; hint: string }> = {
  board: { drawing: board, hint: "Status columns" },
  rows: { drawing: grouped, hint: "Columns, a row per epic" },
  list: { drawing: list, hint: "Lines under each status" },
};

export default function IssuesSettings() {
  const [view, setView] = useSetting(defaultView);
  return (
    <Stack gap="s">
      <Heading>Issues board</Heading>
      <Text tone="muted">The view the Issues tab opens in. You can still switch views in the tab, until you close it.</Text>
      {view === null ? null : (
        <Chips
          label="Default view"
          className="tk-views"
          value={view}
          onChange={(value) => void setView(value)}
          options={DEFAULT_VIEWS.map(({ value, label }) => ({
            value,
            label: (
              <>
                <svg viewBox="0 0 120 62" className="tk-view-pic" aria-hidden="true">
                  {PICTURES[value].drawing}
                </svg>
                <span>{label}</span>
                <span className="tk-view-hint">{PICTURES[value].hint}</span>
              </>
            ),
          }))}
        />
      )}
    </Stack>
  );
}
