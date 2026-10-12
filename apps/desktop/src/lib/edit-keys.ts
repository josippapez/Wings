/**
 * What a macOS terminal sends for its text editing keys, like Terminal and iTerm do: ⌥← and ⌥→ move a word
 * (zsh and bash bind `ESC b` and `ESC f`), ⌘← and ⌘→ go to the start and end of the line, ⌘⌫ deletes to the
 * start. xterm.js sends `ESC [1;3D` for ⌥←, which shells don't bind, and nothing for ⌘←.
 */
export function editKey(e: Pick<KeyboardEvent, "key" | "metaKey" | "altKey" | "ctrlKey" | "shiftKey">): string | null {
  if (e.ctrlKey || e.shiftKey || e.metaKey === e.altKey) return null;
  if (e.altKey) return { ArrowLeft: "\x1bb", ArrowRight: "\x1bf" }[e.key] ?? null;
  return { ArrowLeft: "\x01", ArrowRight: "\x05", Backspace: "\x15" }[e.key] ?? null;
}
