/**
 * Whether the user is in Wings, for the plugins' `onFocus`. Rust reports whether any Wings window is the key
 * window. Moving from one Wings window to another reports false for a moment before true, so a loss of focus
 * only counts once it has held for `SETTLE_MS`; a gain counts at once.
 */
export const SETTLE_MS = 150;

export class FocusTracker {
  private state: boolean;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private onChange: (focused: boolean) => void;

  // No parameter properties: node's type stripping, which runs the tests, doesn't take them.
  constructor(initial: boolean, onChange: (focused: boolean) => void) {
    this.state = initial;
    this.onChange = onChange;
  }

  get focused() {
    return this.state;
  }

  /** The latest report from Rust. */
  report(focused: boolean) {
    if (focused) {
      clearTimeout(this.timer);
      this.timer = undefined;
      this.set(true);
    } else if (this.state && this.timer === undefined) {
      this.timer = setTimeout(() => {
        this.timer = undefined;
        this.set(false);
      }, SETTLE_MS);
    }
  }

  dispose() {
    clearTimeout(this.timer);
  }

  private set(focused: boolean) {
    if (focused === this.state) return;
    this.state = focused;
    this.onChange(focused);
  }
}
