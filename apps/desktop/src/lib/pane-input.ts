/**
 * Remembers, per pane, whether keys you typed there are still waiting in the agent's prompt: text that was never
 * submitted. Wings puts an idle agent to sleep only when nothing is waiting, because ending it would lose the draft.
 * Backspace and cursor keys aren't followed, so a draft you deleted again still counts: when in doubt it is a draft.
 */
export class PaneInput {
  private drafts = new Set<string>();

  note(paneId: string, data: string) {
    // The terminal reports focus changes and mouse moves as input too; those are not typing.
    if (/^\x1b\[[IO]$/.test(data) || /^(\x1b\[<[\d;]*[Mm])+$/.test(data) || /^\x1b\[M[\s\S]{3}$/.test(data)) return;
    // A lone Escape interrupts or closes a menu, it adds nothing to the prompt.
    if (data === "\x1b") return;
    // Enter submits what is typed, Ctrl-C clears it. A paste that ends in Enter is still a paste.
    if (!data.startsWith("\x1b[200~") && (data.endsWith("\r") || data.endsWith("\x03"))) this.drafts.delete(paneId);
    else this.drafts.add(paneId);
  }

  hasDraft(paneId: string) {
    return this.drafts.has(paneId);
  }

  forget(paneId: string) {
    this.drafts.delete(paneId);
  }
}

export const paneInput = new PaneInput();
