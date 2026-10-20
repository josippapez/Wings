/** Quotes one argument for a POSIX shell. Inside single quotes everything is literal except `'` itself. */
// debt: POSIX only (zsh, bash, sh). A Windows pane runs PowerShell or cmd, which quote differently: pick the
// quoting by the pane's shell here once Windows panes resume.
export function shellQuote(arg: string) {
  // A leading `=` is zsh's command path expansion.
  if (/^[\w@%+:,./-][\w@%+=:,./-]*$/.test(arg)) return arg;
  return `'${arg.replaceAll("'", `'\\''`)}'`;
}

/** Which agent CLI a session belongs to. */
export type SessionKind = "claude" | "opencode";

/**
 * How each agent CLI continues a session, as argv. Adding an agent is an entry here. `sessionId` is the shape of its
 * ids, which get typed into a shell, so nothing else is accepted. `fork` is left out when the CLI has no fork.
 */
const CLIS: Record<SessionKind, { sessionId: RegExp; resume: (id: string, flags: readonly string[]) => string[]; fork?: (id: string) => string[] }> = {
  // `--resume` plus the flags the session was started with.
  claude: { sessionId: /^[0-9a-f-]{36}$/i, resume: (id, flags) => ["claude", "--resume", id, ...flags], fork: (id) => ["claude", "--resume", id, "--fork-session"] },
  // `opencode --help`: `--session` continues a session. It keeps no start flags worth carrying over.
  opencode: { sessionId: /^ses_[A-Za-z0-9]{8,}$/, resume: (id) => ["opencode", "--session", id] },
};

export const isSessionIdOf = (kind: SessionKind, id: string) => CLIS[kind].sessionId.test(id);

/** The line that continues a session in its pane's shell, with the flags it was started with where the CLI takes them. */
export function resumeLine(kind: SessionKind, sessionId: string, args: readonly string[] = []) {
  // The line is typed into a terminal, where a control character acts as a key press, so those flags stay out.
  const flags = args.some((a) => /[\x00-\x1f\x7f]/.test(a)) ? [] : args;
  return CLIS[kind].resume(sessionId, flags).map(shellQuote).join(" ") + "\r";
}

/** The line typed into a restored pane: `claude --resume <id>` and the flags the session was started with. */
export const resumeCommand = (sessionId: string, args: readonly string[] = []) => resumeLine("claude", sessionId, args);

/** Whether the agent can copy a session into a new one. */
export const canFork = (kind: SessionKind) => !!CLIS[kind].fork;
/** The line that starts a copy of a session, or null when the agent can't fork. */
export const forkLine = (kind: SessionKind, sessionId: string) => CLIS[kind].fork?.(sessionId).map(shellQuote).join(" ").concat("\r") ?? null;

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
