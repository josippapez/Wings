// Small text formatters that several plugins had a copy of.

/** "just now", "5 min ago", "3 h ago", "2 d ago", then the date. "" for something that isn't a date. */
export function ago(iso, now = Date.now()) {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const minutes = Math.max(0, Math.round((now - t) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  if (minutes < 1440) return `${Math.round(minutes / 60)} h ago`;
  if (minutes < 43_200) return `${Math.round(minutes / 1440)} d ago`;
  return new Date(t).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/** `n` with its noun: "1 file", "2 files". `many` is for a plural that isn't `one` and an s: `plural(2, "child", "children")`. */
export const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The last part of a path, like `repo` for `/Users/me/repo/`. A path with no parts, like `/`, comes back as it is. */
export const basename = (path) => path.split("/").filter(Boolean).at(-1) ?? path;

/** `text` cut to `max` characters, ending in "…" when it was longer. */
export const truncate = (text, max) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);
