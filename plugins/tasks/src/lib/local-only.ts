// Keep issues local: finds a Wings issue key in a command that writes a commit or a pull request, so the hook can ask for it to be removed.

const GIT = String.raw`\bgit(?:\s+(?:-[cC]\s+\S+|--?[\w-]+(?:=\S+)?))*`;
const WRITES = [
  new RegExp(String.raw`${GIT}\s+commit\b`),
  new RegExp(String.raw`${GIT}\s+tag\b[^\n;&|]*\s-[A-Za-z]*[am]\b`),
  /\bgh\s+pr\s+(?:create|edit|comment|review)\b/,
  /\bgh\s+issue\s+(?:create|comment)\b/,
  /\bglab\s+mr\s+(?:create|update|note)\b/,
  /\baz\s+repos\s+pr\s+(?:create|update)\b/,
];

/** Whether the command commits, tags or writes to a pull request or issue on a host. */
export const writesToHistory = (command: string): boolean => WRITES.some((re) => re.test(command));

/** The first issue key with one of `prefixes` that the command holds as a whole word, or null: `WEB-12` yes, `XWEB-12` and `web-12` no. */
export function issueKeyIn(command: string, prefixes: string[]): string | null {
  if (!prefixes.length) return null;
  return command.match(new RegExp(String.raw`(?<![A-Za-z0-9])(?:${prefixes.join("|")})-\d{1,9}(?![A-Za-z0-9])`))?.[0] ?? null;
}

/** The reason to deny a command, or null when it may run: it writes to history and holds a key of a tracked project. */
export function localOnlyDenial(command: string, prefixes: string[]): string | null {
  if (!writesToHistory(command)) return null;
  const key = issueKeyIn(command, prefixes);
  return key ? `${key} is a local Wings issue: remove the key from the message, then run it again.` : null;
}
