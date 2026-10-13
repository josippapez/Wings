---
name: wings-page-design
description: Lays out and checks a Wings page (a plugin tab, view, section or block, or a page in the app) so nothing ships unstyled, unpadded or outside the page frame. Use before adding or moving any control, header, back button, toolbar, list or empty state on a page, and before saying a page change is done. Also use when the user says a page looks ugly, primitive, cramped, misaligned or "without any sensible design".
---

# Wings page design

A page change isn't done until you've looked at a render of it. Tests and type checks don't see layout.

## Layout rules

Plugin pages use `wings-plugin-ui`. Its components carry the padding, spacing and theme, so a page only places them.

1. **Everything sits inside `Page`.** The element passed to `showTab` or `showView` is one `Page` (or `Empty`/`Loading`). Never render a `Row`, `Stack` or button as a sibling above or around `Page`. It lands in the frame's top-left corner with no padding.
2. **Page-level controls go in the `Toolbar`.**
   - `title` is the page name.
   - `children` hold navigation and filters. A back link is a ghost `Button` reading `← <list name>` followed by the current item, as a breadcrumb.
   - `actions` hold buttons on the right, with at most one solid primary button.
3. **One header per page.** Don't repeat the same name or number in the toolbar and the body heading. The toolbar names the place, and the body heading names the thing.
4. **No hand-made chrome.**
   - Use plugin-ui components, not raw `<button>` or `<input>` elements.
   - Use `var(--wings-*)` tokens, not hex colours.
   - Use the page's own spacing, not ad hoc margins.
   - Page CSS only lays out the page's own blocks.
5. **Width.** Reading pages centre a column of at most about 860px. Lists fill the width. At 640px the toolbar must still fit: actions wrap or collapse into a `Menu`, and they're never clipped.
6. **States.** Each page has a designed loading, empty and error state, using `Loading`, `Empty` and an error card.
7. **App pages** follow the shadcn components in `apps/desktop/src/components/ui` and the direction in memory `wings-ui-direction`. They don't use one-off styles.

## Check before done

1. **Fixture.** Add or update a fixture in `plugins/<plugin>/preview/<name>.tsx` with made-up data. It should cover each state you touched: navigated in, opened directly, empty, and error.
2. **Render.** Run `pnpm --dir plugins shot <plugin>` and read every PNG it prints. It renders at 1000px and 640px.
3. **Compare.** Hold the page against the app around it.
   - Padding matches the toolbar.
   - Controls sit on the same baseline.
   - Nothing touches the frame edge.
   - Nothing is clipped or overlaps.
   - Text is no smaller than the app's.
4. **Fix and re-render.** Write down every problem you see, fix it, and render again. "Looks fine" without having opened the PNG doesn't count.
5. **Report.** Give the PNG paths in your report.

For token drift between the app and plugin-ui, run `pnpm --dir apps/desktop parity`.
