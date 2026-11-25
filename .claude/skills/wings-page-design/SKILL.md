---
name: wings-page-design
description: Lays out and checks a Wings page (a plugin tab, view, section or block, or a page in the app) so nothing ships unstyled, unpadded or outside the page frame. Use before adding or moving any control, header, back button, toolbar, list or empty state on a page, and before saying a page change is done. Also use when the user says a page looks ugly, primitive, cramped, misaligned or "without any sensible design".
---

# Wings page design

A page change isn't done until you've looked at a render of it. Tests and type checks don't see layout.

The design system, which control to pick and where it goes, is in [design-system.md](design-system.md). Read it before
laying out a page.

## Layout rules

Plugin pages are built with `wings_plugin::ui` (`packages/plugin-rs/src/ui.rs`). The app draws each node with its own controls, which carry the padding, spacing and theme, so a page only places them.

1. **Everything sits inside `ui::page`.** `view` returns one `page(toolbar, body)` (or `empty`/`loading`). Never return a `row`, `stack` or button as a sibling above or around it. It lands in the frame's top-left corner with no padding.
2. **Page-level controls go in the toolbar.**
   - `toolbar(title, actions)` and `toolbar_with(title, children, actions)`: `title` is the page name.
   - `children` hold navigation and filters. A back link is a ghost `button` reading `← <list name>` followed by the current item, as a breadcrumb.
   - `actions` hold buttons on the right, with at most one solid primary button.
   - A header with a context after the name is `toolbar_titled(title_with_context(name, context), actions)`; a page with a header and bars that stay put is `page_with(top, body)`; a ghost icon button is `icon_button(label, &icons::NAME, on_click)` (Lucide paths in `ui::icons`), and `.size`, `.aria_label`, `.tooltip`, `.disabled` and `.segmented` set the props a button or a `chips` takes. A settings row is `setting(name, detail, control)` or `setting_tick(checkbox, detail)`; a line of facts is `joined`, a small label over a section `section_label`.
3. **One header per page.** Don't repeat the same name or number in the toolbar and the body heading. The toolbar names the place, and the body heading names the thing.
4. **No hand-made chrome.**
   - Use the `ui` functions, not a `Node::new("<kind>")` for something `ui` already has.
   - Use a `tone` or variant, not a colour of your own.
   - Use the page's own spacing, not ad hoc padding.
5. **Width.** Reading pages centre a column of at most about 860 points. Lists fill the width. The toolbar wraps its actions onto a second line when it is narrow (42f62126), so at 640 points they are never clipped.
6. **States.** Each page has a designed loading, empty and error state, using `loading`, `empty` and a `notice` card.
7. **Tooltips.** A `button` or `text` that is only an icon, or whose text is cut short, takes a `title` prop (`.prop("title", "...")`), which the app shows as a tooltip on hover (4dbefc8d).
8. **App pages** (Settings, the plugin manager) follow the controls in `apps/native/src/ui` and the direction in memory `wings-ui-direction`. They don't use one-off styles.
9. **Popups.** A menu, select list, combobox list, hover card or any card that opens beside a trigger in `apps/native` is built from the behaviors in `apps/native/src/ui/behaviors` (`popup`/`popup_at`/`popup_on_parent` for placement, `bind_layer` for Escape and press-outside, `FocusRecovery`, `listbox`), or from `ui::dropdown_menu`/`ui::context_menu`, which use them. Never hand-build one with `anchored()`/`deferred()`: each hand-built popup placed and closed itself differently, and opened off the window's foot (see `apps/native/README.md`).

## Check before done

1. **Scene.** Add or update the plugin's scene in `apps/native/examples/shot_plugins.rs` with made-up data. It has a state for each state you touched: navigated in, opened directly, empty, loading and error. Each state is one screenshot. A new plugin gets a row in `SCENES` (its flag, a line of help and its function) and a function like the existing ones (`changes`, `hud`, `disk`...); `-- <out-dir> --list` prints every flag. It runs the committed module, so build it first: `sh scripts/build.sh sh plugins/build-rust.sh <plugin>`.
2. **Render.** Run `sh scripts/build.sh pnpm --dir plugins shot <out-dir> --<plugin>` (it runs `cargo run --manifest-path apps/native/Cargo.toml --example shot_plugins --features test-support -- <out-dir> --<plugin>`) and open every PNG it writes. A page is drawn at 1000 and 640 points wide. macOS only. Claude chat's run is long: `CHAT_SCENE=<name>[,<name>...]` before the command renders only the scenes you touched, and `CHAT_SKIP=<name>` steps past one known to fail. App pages render with `examples/shot.rs`: `sh scripts/build.sh cargo run --manifest-path apps/native/Cargo.toml --example shot --features test-support -- <out-dir> <scene>` (the scene names are the `nth(2)` arms in its `scenes`, like `plugins-install`). In a `shot.rs` scene, `wait` moves only the app's clock (animations, timers) and costs no real time, and `settle` waits in real time until shells, git and plugin pages go quiet: use `settle` after opening a window, typing into a shell or opening a plugin page, and `wait` for the rest.
   - **Read the cut-off lines.** Both examples check every screen for what it cuts off at its width (`apps/native/src/ui/cutoff.rs`): a line `CUT OFF at <width> in <png>: <what> <how>` under that PNG for each text, button or box that runs past the window's edge, the page's or a box that clips it ("runs 38 pt past the window's right edge", "is wholly past ..."), text or a button past its card or its own box, and page text squeezed to a few letters a line. They are listed again at the end and the run exits with 1; a clean run ends `Nothing cut off.` `<what>` is the words or the button's name, or a box by size and place in points from the window's top left (the PNG is twice that in pixels). Open that PNG at that place and fix the page. Text cut short with an ellipsis and what is in a box that scrolls sideways don't count; a box that clips on purpose takes `.children(ui::cutoff::clips_on_purpose())`. The check doesn't replace looking: it reads page text, buttons, cards and painted boxes, not a terminal, a text field or an app page's plain text.
3. **Compare.** Hold the page against the app around it.
   - Padding matches the toolbar.
   - Controls sit on the same baseline.
   - Nothing touches the frame edge, a split's divider or the card beside it. A `Split` whose panes hold cards sets `gap: "l"`.
   - Nothing is clipped or overlaps, and no name or text is squeezed to a few letters a line beside a control.
   - Text is no smaller than the app's.
4. **Fix and re-render.** Write down every problem you see, fix it, and render again. "Looks fine" without having opened the PNG doesn't count.
5. **Every page you touched, every width.** A change made by a subagent counts too: open its renders yourself before a release, at 640 as well as 1000. Issues' settings shipped with its name squeezed to a letter a line at 640 because nobody opened that render.
6. **Report.** Give the PNG paths in your report.
