/**
 * One or more replies xterm sends for a program, like Claude Code asking at start: device attributes (`ESC[?6c`,
 * `ESC[>0;276;0c`), a status or mode report (`ESC[0n`, `ESC[?2026;2$y`), and OSC or DCS answers such as a color.
 */
const TERMINAL_REPLY = /^(?:\x1b\[[?>][\d;]*c|\x1b\[\d*n|\x1b\[\??[\d;]*\$y|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1bP[^\x1b]*\x1b\\)+$/;

/**
 * Remembers, per pane, whether keys you typed there are still waiting in the agent's prompt: text that was never
 * submitted. Wings puts an idle agent to sleep only when nothing is waiting, because ending it would lose the draft.
 * Backspace and cursor keys aren't followed, so a draft you deleted again still counts: when in doubt it is a draft.
 * A message another agent or a plugin left there unsent waits too, but it is no draft of yours.
 */
export class PaneInput {
  private drafts = new Set<string>();
  private messages = new Set<string>();

  note(paneId: string, data: string) {
    // The terminal reports focus changes and mouse moves as input too; those are not typing.
    if (/^\x1b\[[IO]$/.test(data) || /^(\x1b\[<[\d;]*[Mm])+$/.test(data) || /^\x1b\[M[\s\S]{3}$/.test(data)) return;
    // The terminal's answers to a program's questions (its version, colors, a mode) reach the pane as input too.
    if (TERMINAL_REPLY.test(data)) return;
    // A lone Escape interrupts or closes a menu, it adds nothing to the prompt.
    if (data === "\x1b") return;
    // Enter submits what is typed, Ctrl-C clears it. A paste that ends in Enter is still a paste.
    if (!data.startsWith("\x1b[200~") && (data.endsWith("\r") || data.endsWith("\x03"))) {
      this.drafts.delete(paneId);
      this.messages.delete(paneId);
    } else this.drafts.add(paneId);
  }

  /** A message Wings pasted for another agent or a plugin, left in the prompt for you to send. */
  noteMessage(paneId: string) {
    this.messages.add(paneId);
  }

  /** Something waits in the prompt: your typing or a message left there. */
  hasDraft(paneId: string) {
    return this.drafts.has(paneId) || this.messages.has(paneId);
  }

  /** Only what you typed: a message from an agent can go on top of another. */
  hasTyped(paneId: string) {
    return this.drafts.has(paneId);
  }

  /** The prompt was sent by Wings itself, with everything in it. */
  sent(paneId: string) {
    this.drafts.delete(paneId);
    this.messages.delete(paneId);
  }

  forget(paneId: string) {
    this.sent(paneId);
  }
}

export const paneInput = new PaneInput();
