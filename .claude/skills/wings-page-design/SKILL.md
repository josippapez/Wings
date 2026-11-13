---
name: wings-page-design
description: Lays out and checks a Wings page (a plugin tab, view, section or block, or a page in the app) so nothing ships unstyled, unpadded or outside the page frame. Use before adding or moving any control, header, back button, toolbar, list or empty state on a page, and before saying a page change is done. Also use when the user says a page looks ugly, primitive, cramped, misaligned or "without any sensible design".
---

# Wings page design

A page change isn't done until you've looked at a render of it. Tests and type checks don't see layout.

## Layout rules

Plugin pages are built with `wings_plugin::ui` (`packages/plugin-rs/src/ui.rs`). The app draws each node with its own controls, which carry the padding, spacing and theme, so a page only places them.

1. **Everything sits inside `ui::page`.** `view` returns one `page(toolbar, body)` (or `empty`/`loading`). Never return a `row`, `stack` or button as a sibling above or around it. It lands in the frame's top-left corner with no padding.
2. **Page-level controls go in the toolbar.**
   - `toolbar(title, actions)` and `toolbar_with(title, children, actions)`: `title` is the page name.
   - `children` hold navigation and filters. A back link is a ghost `button` reading `← <list name>` followed by the current item, as a breadcrumb.
   - `actions` hold buttons on the right, with at most one solid primary button.
3. **One header per page.** Don't repeat the same name or number in the toolbar and the body heading. The toolbar names the place, and the body heading names the thing.
4. **No hand-made chrome.**
   - Use the `ui` functions, not a `Node::new("<kind>")` for something `ui` already has.
   - Use a `tone` or variant, not a colour of your own.
   - Use the page's own spacing, not ad hoc padding.
5. **Width.** Reading pages centre a column of at most about 860 points. Lists fill the width. The toolbar wraps its actions onto a second line when it is narrow (42f62126), so at 640 points they are never clipped.
6. **States.** Each page has a designed loading, empty and error state, using `loading`, `empty` and a `notice` card.
7. **Tooltips.** A `button` or `text` that is only an icon, or whose text is cut short, takes a `title` prop (`.prop("title", "...")`), which the app shows as a tooltip on hover (4dbefc8d).
8. **App pages** (Settings, the plugin manager) follow the controls in `apps/native/src/ui` and the direction in memory `wings-ui-direction`. They don't use one-off styles.

## Check before done

1. **Scene.** Add or update the plugin's scene in `apps/native/examples/shot_plugins.rs` with made-up data. It has a state for each state you touched: navigated in, opened directly, empty, loading and error. Each state is one screenshot. A new plugin gets a flag and a function like the existing ones (`changes`, `hud`, `disk`...). It runs the committed module, so run `sh build-rust.sh` in `plugins/` first.
2. **Render.** Run `pnpm --dir plugins shot <out-dir> --<plugin>` (it runs `cargo run --manifest-path apps/native/Cargo.toml --example shot_plugins --features test-support -- <out-dir> --<plugin>`) and open every PNG it writes. A page is drawn at 1000 and 640 points wide. macOS only.
3. **Compare.** Hold the page against the app around it.
   - Padding matches the toolbar.
   - Controls sit on the same baseline.
   - Nothing touches the frame edge.
   - Nothing is clipped or overlaps.
   - Text is no smaller than the app's.
4. **Fix and re-render.** Write down every problem you see, fix it, and render again. "Looks fine" without having opened the PNG doesn't count.
5. **Report.** Give the PNG paths in your report.
