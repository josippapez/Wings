# Terminal settings

Settings most native terminals have and Wings doesn't yet. They go in the Settings sheet (`apps/desktop/src/components/settings-sheet.tsx`) and are saved in `SavedWorkspace` in `apps/desktop/src/App.tsx`, like `terminalKeys`.

## Already in Wings

- Font size, with the zoom shortcuts.
- Use Option as Meta: off, left, right or both. Default left.
- Shift-Return sends Meta Return. Default on.

## Next

Option names and line numbers are from xterm.js 6.0.0, `node_modules/@xterm/xterm/typings/xterm.d.ts`.

| Setting | xterm.js | Wings today | Default |
|---|---|---|---|
| Font family | `fontFamily` (123), `fontWeight` and `fontWeightBold` (128, 133), `letterSpacing` (145) | Hardcoded mono stack in `apps/desktop/src/lib/terminal.ts` | A text field put in front of the current stack |
| Line height | `lineHeight` (150) | Hardcoded 1.2 | 1.2, range 1.0 to 1.6 |
| Cursor style and blink | `cursorStyle` (68), `cursorBlink` (63), `cursorInactiveStyle` (78) | Block, blinking | Block, blink on, inactive `outline` |
| Scrollback lines | `scrollback` (251) | Hardcoded 10,000 | 10,000, presets from 1k to 100k |
| Copy on select | No option. `onSelectionChange` (996) plus `getSelection()` (1168) | None | Off |
| Minimum contrast | `minimumContrastRatio` (207) | Unset, so 1 | 1, with 4.5 offered for readability |

## Later

- **Bell.** xterm.js only fires `onBell` (917). Wings draws the badge, flash or sound itself. Default: a tab badge, no sound.
- **Multi-line paste warning.** No xterm.js option. Catch the paste before `term.paste` and warn only when bracketed paste mode is off.
- **Theme.** `theme` (285). The background must stay opaque because of the WebGL renderer, see the comment on `theme.background` in `apps/desktop/src/lib/terminal.ts`.
- **Word separators** for double-click: `wordSeparator` (309).
- **Scroll speed:** `scrollSensitivity` (269), `fastScrollSensitivity` (113).
- **Ligatures:** needs `@xterm/addon-ligatures`. Check that it works with the WebGL renderer first.
- **Find in terminal:** needs `@xterm/addon-search`.

## How to build them

- Put each value in a module-level setting in `apps/desktop/src/lib/terminal.ts` with a setter, like `setTerminalKeys`. Apply it to every open terminal when it changes. Never read storage in the key handler.
- Font family, line height, letter spacing and font weight change the cell size. Each one remeasures, refits and resizes the PTY of every pane. Follow the `wings-performance` skill. Apply the change once to all panes, debounce sliders, and measure a change with several panes open before and after.
- Smaller scrollback drops history. Say so next to the setting.
- Copy on select writes to the clipboard on mouse-up, not on every `onSelectionChange`, which fires while you drag.

## Check

- `cd apps/desktop && pnpm exec tsc --noEmit`
- In the browser stub (see `.claude/skills/wings-performance/SKILL.md`), change each setting and read `term.options` on an open terminal. Reload and check the value is restored.
