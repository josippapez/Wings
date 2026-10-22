// What a plugin may ask of `wings.diskUsage` and `wings.diskFind`, checked before the call reaches Rust. Plugin data is
// untrusted, so a call that breaks a limit is refused with a message that names what is wrong. Rust clamps the ranges.

/** The token that names one scan or search, so the plugin can stop it. */
export function diskToken(v: unknown) {
  if (typeof v !== "string" || v.length < 1 || v.length > 64) throw new Error("A scan needs a token of 1 to 64 characters");
  return v;
}

/** A number a plugin gave for `wings.diskUsage`, 0 or more and at most `max`, or null when it gave none. */
function diskCount(name: string, v: unknown, max: number, whole: boolean) {
  if (v === undefined || v === null) return null;
  if (typeof v !== "number" || v < 0 || !(whole ? Number.isSafeInteger(v) : Number.isFinite(v))) {
    throw new Error(`diskUsage ${name} must be ${whole ? "a whole number" : "a number"}, 0 or more`);
  }
  // A size is whole bytes, so a floor of 1.5 is 2. The cap only keeps it in the integer Rust reads: Rust clamps the ranges.
  return Math.min(whole ? v : Math.ceil(v), max);
}

/** The `wings.diskUsage` call as Rust takes it, or an error saying what the plugin got wrong. */
export function diskUsageRequest(p: Record<string, unknown>) {
  if (typeof p.path !== "string" || !p.path) throw new Error("diskUsage needs a path");
  return {
    path: p.path,
    minBytes: diskCount("minBytes", p.minBytes, Number.MAX_SAFE_INTEGER, false),
    depth: diskCount("depth", p.depth, 2 ** 32 - 1, true),
    maxEntries: diskCount("maxEntries", p.maxEntries, 2 ** 32 - 1, true),
    token: diskToken(p.token),
    stream: p.stream === true,
  };
}

/** `min` to `max` names, each 1 to 255 characters (the longest a file name is) with no `/`. A list left out is empty. */
function diskNames(field: string, v: unknown, min: number, max: number): string[] {
  const list = v === undefined || v === null ? [] : v;
  if (!Array.isArray(list) || list.length < min || list.length > max) throw new Error(`diskFind ${field} must be a list of ${min} to ${max} names`);
  if (!list.every((name) => typeof name === "string" && name.length >= 1 && name.length <= 255 && !name.includes("/"))) {
    throw new Error(`diskFind ${field} must be names of 1 to 255 characters with no /`);
  }
  return list;
}

/** The `wings.diskFind` call as Rust takes it, or an error saying what the plugin got wrong. */
export function diskFindRequest(p: Record<string, unknown>) {
  if (typeof p.path !== "string" || !p.path) throw new Error("diskFind needs a path");
  return {
    path: p.path,
    names: diskNames("names", p.names, 1, 16),
    skip: diskNames("skip", p.skip, 0, 32),
    token: diskToken(p.token),
    stream: p.stream === true,
  };
}
