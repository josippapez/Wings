export { cn } from "cn"

/** A path to type into a shell, single-quoted. */
export const shellQuote = (path: string) => `'${path.replaceAll("'", `'\\''`)}'`;
