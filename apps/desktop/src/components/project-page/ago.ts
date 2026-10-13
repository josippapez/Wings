const units: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 86_400_000],
  ["month", 30 * 86_400_000],
  ["day", 86_400_000],
  ["hour", 3_600_000],
  ["minute", 60_000],
];
const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** "2 hours ago", "yesterday", "just now": how long before `now` a time was. The exact time goes in a title. */
export function ago(at: number, now = Date.now()): string {
  const diff = at - now;
  for (const [unit, ms] of units) if (Math.abs(diff) >= ms) return relative.format(Math.trunc(diff / ms), unit);
  return "just now";
}
