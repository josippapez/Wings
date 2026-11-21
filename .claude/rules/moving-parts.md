# Moving parts draw in a layer

Anything in the app that changes while the page's tree stays the same draws in a window layer, never inside a workspace piece or a page: a spinner, a pulse, a counting clock, typed text and its caret, a progress sweep, an animated background. A step or a key redraws its layer and nothing else.

GPUI redraws a notified view and every view above it, and a cached piece that redraws rebuilds everything inside it. One "Running · 12s" in a chat page rebuilt the whole page every second, and a key in the message box rebuilt the main column.

- **Use the layer that exists.** Spinners and working dots go in `ui::view_spinner_slot` in a page, or the native spinners. Pulsing text goes in `ui::shimmer_slot`, and counting text in `ui::count_slot` (a page `Text`'s `since`). A `TextField` draws its text in the field layer by itself. Backgrounds go in a `backdrop` slot.
- **Never notify a page, a piece or the Workspace for a step.** That means no repeating `with_animation` and no timer that calls `cx.notify()` on them. Notify the layer's own view.
- **Plugins** show time with a `Text`'s `since`, and motion with `shimmer`, `Spinner` and `Dot`. They never send a page's tree again on a timer to move something. After a key, the page sends back only what was typed.
- **Something new that moves and fits none of these** gets a layer in `apps/native/src/ui/slots.rs` first, shared, then used.
- **Prove it** with a test that counts the page's and the piece's renders across clock steps or keys and expects 0, like the `spinners` and `field_layer` tests in `apps/native/src/page/tests.rs`, failing on the old code. Then measure it with `wings performance` in the installed app.

The details and the measuring method are in the `wings-performance` skill.
