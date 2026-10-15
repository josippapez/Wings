/** Dates as the History list and the graph show them. Every `at` is Unix seconds, as git prints it. */

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 31_557_600],
  ["month", 2_629_800],
  ["week", 604_800],
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];
/** Like "3 hours ago". */
export function ago(at: number) {
  const s = at - Date.now() / 1000;
  const unit = UNITS.find(([, size]) => Math.abs(s) >= size);
  return unit ? relative.format(Math.round(s / unit[1]), unit[0]) : "just now";
}

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const dayTime = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
/** Like "15 Oct 2026". */
export const dateOf = (at: number) => day.format(at * 1000);
/** Like "15 Oct 2026, 14:03". */
export const dateTimeOf = (at: number) => dayTime.format(at * 1000);
