/** "just now", "5 min ago", "3 h ago", "2 d ago", then the date. "" for something that isn't a date. */
export function ago(iso: string, now?: number): string;

/** `n` with its noun: "1 file", "2 files". `many` is for a plural that isn't `one` and an s: `plural(2, "child", "children")`. */
export function plural(n: number, one: string, many?: string): string;

/** The last part of a path, like `repo` for `/Users/me/repo/`. A path with no parts, like `/`, comes back as it is. */
export function basename(path: string): string;

/** `text` cut to `max` characters, ending in "…" when it was longer. */
export function truncate(text: string, max: number): string;
