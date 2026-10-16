// Small text formatters that several plugins had a copy of.

/**
 * "just now", "5 min ago", "3 h ago", "2 d ago", then the date. "" for something that isn't a date. `when` is an ISO
 * string, a `Date`, or milliseconds since 1970, so a time in seconds, like git's, is `ago(seconds * 1000)`.
 */
export function ago(when, now = Date.now()) {
  const t = typeof when === "string" ? Date.parse(when) : when instanceof Date ? when.getTime() : when;
  if (typeof t !== "number" || Number.isNaN(t)) return "";
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

/** A token count in k and M: `999`, `116k`, `1.2M`. Thousands are whole, and millions have one decimal unless it is 0: `1M`. */
export const tokens = (n) => (n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));

/** The time of day of `ms`, in the user's locale: `09:41` or `09:41 AM`. */
export const clock = (ms) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
