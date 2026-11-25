# Terminal settings

Settings most native terminals have and Wings doesn't yet. They go in Settings (`apps/native/src/workspace/settings.rs`, the terminal sections) and are saved in the workspace file (`apps/native/src/saved.rs`, next to `option_as_meta`).

## Already in Wings

- Font size, with the zoom shortcuts.
- Use Option as Meta: off, left, right or both. Default left.
- Shift-Return sends Meta Return. Default on.
- Font family, line height (1.0 to 1.6), cursor style, blink and inactive style, scrollback (1k to 100k, with the memory cost and the history drop said next to it), copy on select (on mouse-up) and minimum contrast (off, 4.5, 7). Font weight and letter spacing are not exposed yet.

## Later

- **Bell.** Wings draws the badge, flash or sound itself. Default: a tab badge, no sound.
- **Multi-line paste warning.** Warn only when bracketed paste mode is off.
- **Word separators** for double-click.
- **Scroll speed.**
- **Ligatures.**
- **Find in terminal.**
- **Font weight and letter spacing.**

## How to build them

- Put each value in `saved.rs` and apply it to every open terminal when it changes (`apps/native/src/terminal_view`). Never read the saved file in the key handler.
- Font family, line height, letter spacing and font weight change the cell size. Each one remeasures, refits and resizes the PTY of every pane. Follow the `wings-performance` skill. Apply the change once to all panes, debounce sliders, and measure a change with several panes open before and after.
- Smaller scrollback drops history. Say so next to the setting.
- Copy on select writes to the clipboard on mouse-up, not while you drag.
