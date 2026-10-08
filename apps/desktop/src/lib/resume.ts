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
