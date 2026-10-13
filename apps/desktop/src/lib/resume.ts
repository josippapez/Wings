/** Quotes one argument for a POSIX shell. Inside single quotes everything is literal except `'` itself. */
// debt: POSIX only (zsh, bash, sh). A Windows pane runs PowerShell or cmd, which quote differently: pick the
// quoting by the pane's shell here once Windows panes resume.
export function shellQuote(arg: string) {
  // A leading `=` is zsh's command path expansion.
  if (/^[\w@%+:,./-][\w@%+=:,./-]*$/.test(arg)) return arg;
  return `'${arg.replaceAll("'", `'\\''`)}'`;
}

/** The line typed into a restored pane: `claude --resume <id>` and the flags the session was started with. */
export function resumeCommand(sessionId: string, args: readonly string[] = []) {
  // The line is typed into a terminal, where a control character acts as a key press, so those flags stay out.
  const flags = args.some((a) => /[\x00-\x1f\x7f]/.test(a)) ? [] : args;
  return ["claude", "--resume", ...[sessionId, ...flags].map(shellQuote)].join(" ") + "\r";
}

/** Which agent CLI a session belongs to. */
export type SessionKind = "claude" | "opencode";

/** Session ids get typed into a shell, so only the shapes the CLIs use are accepted: Claude Code's UUID, opencode's `ses_…`. */
export const isSessionIdOf = (kind: SessionKind, id: string) => (kind === "claude" ? /^[0-9a-f-]{36}$/i.test(id) : /^ses_[A-Za-z0-9]{8,}$/.test(id));

/** The line that continues a session in its pane's shell. opencode: `opencode --session <id>` (`opencode --help`). */
export function resumeLine(kind: SessionKind, sessionId: string, args: readonly string[] = []) {
  return kind === "claude" ? resumeCommand(sessionId, args) : `opencode --session ${shellQuote(sessionId)}\r`;
}

const SHELLS = new Set(["sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh"]);

/** True when a pane's foreground program (`PaneInfo.command`) is a shell, so a typed line reaches its prompt and not a program. */
export const isShellCommand = (command: string | undefined) => SHELLS.has(command ?? "");

/** The folder each pane's shell is in, or was in when last saved if its shell hasn't started this launch. */
export function paneCwds(panes: Record<string, { paneId: string | null; savedCwd?: string | null }>, info: Record<string, { cwd: string | null }>) {
  return Object.fromEntries(
    Object.entries(panes).flatMap(([key, meta]) => {
      const cwd = (meta.paneId ? info[meta.paneId]?.cwd : null) ?? meta.savedCwd;
      return cwd ? [[key, cwd]] : [];
    }),
  );
}
