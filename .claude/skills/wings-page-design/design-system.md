# Wings design system

What Claude chat, the app's sidebar and tabs, and Settings already do, written down so every page does the same. A page
built from `wings_plugin::ui` gets the colours, type and spacing from the app; this file says which control to pick and
where it goes.

## Ground and colour

The app's palette (`apps/native/src/theme.rs`):

| Role | Use it for |
| --- | --- |
| Foreground | Names, titles, what you read first. |
| Muted | Context after a title (`folder · branch`), descriptions, counts, times, "Show 14 more". |
| Hairline | Lines between rows in a card, under a page header, at a split's divider. Never a heavier line. |
| Hover / pressed | A row or ghost button under the pointer, a selected chip. |
| Info (blue) | In progress, links, a count badge on a button. |
| Working (amber) | In review, waiting, a pause. |
| Done (green) | Done, success, added lines. |
| Danger (red) | Errors, blocked, a stop, removed lines. |
| Claude orange | The Claude spark only. |

Colour comes from a `tone`, never a hex of a page's own. A status is a dot or a small badge in its tone, not coloured
text.

## Type

- **Title**: the body size in a strong weight (the chat's session name, a toolbar title). Pages have no big headings.
- **Body**: the default size, for what you read.
- **Small muted**: context, descriptions, meta. A setting's description, a row's time, the folder after a title.
- **Mono**: keys and code only (`WEB-4`, a path, a command).

## Page header

One line at the top, as Claude chat's header and a terminal pane's:

- **Left**: the page's name strong, then its context muted and small (`Issues  tanstack-router-example`, `Claude chat
  wings · main`). Never "Name, context" in one string.
- **Right**: actions as ghost icon buttons (`icon-sm`) with a tooltip and, for the common ones, a shortcut. Creating
  something is a `+` icon button ("New chat", "New issue"), not a text field in the header. A search field sits here at
  a fixed width (220) when the page is searched.
- A page that lists things puts its view controls in a **filter bar** under the header, not in it.

## Filter bar

A second line under the header that doesn't scroll, for how the list is shown:

- **View** (2 or 3 ways to show the same things): a segmented track (`chips` with `segmented`), like Settings' tracks.
- **Sort**: a segmented track after a muted "Sort" label for up to 4 orders, else a menu (`select`, 220 wide).
- **On/off** ("Group by parent", "Show canceled"): a checkbox, as Settings' booleans.
- **Counts** of the whole list: small badges in their tones at the bar's end.

Exclusive choices never look like plain buttons: a row of ghost buttons where one is "on" reads as actions.

## Choosing a control

| You pick | Control |
| --- | --- |
| One of 2 to 5 short options | Segmented track |
| One of more, or long names | Menu (`select`), 220 wide, a ghost one in a toolbar or message box |
| On or off | Checkbox with its name as the label |
| An action | Button: ghost in bars, outline or secondary in content, one solid primary per view at most |
| An action on one thing | Ghost icon button with a tooltip |

## Lists and sections

As the app's sidebar:

- A section is a small muted label (with its count, and "All" at the right end when there is more).
- A row is a lead in a 16-point box (status dot, icon, avatar), the name that truncates, and muted meta at the end.
  Rows have no borders; the hover tint shows the row.
- A long list shows its first rows and a muted "Show N more".

## Cards and settings rows

As Settings:

- A group is a section heading over one card whose rows are split by hairlines (`rows_card`).
- A row is a name (medium), a small muted description under it, and its control at the end (`entry`). A control too
  wide for the rest goes under the words; never squeeze the name.
- Every option that changes what you see has a preview card under it showing what it changes.

## Layout and spacing

- Gaps: `xs` 2, `s` 4, `m` 8, `l` 12 points. Inside a row `s` or `m`; between sections `l`.
- A page's padding is the app's (14). Nothing touches the frame, a divider or the card beside it: a `Split` holding cards
  sets `gap: "l"`.
- Reading content is a column of at most 760 to 860 points, centred; lists fill the width.
- Floating things (the chat's message box, its session panel, popovers) are raised cards with the app's radius.

## Motion

Anything that moves (spinners, counts, shimmers, cues, backdrops) draws in a live layer, never by redrawing the page
(`.claude/rules/moving-parts.md`). Claude chat's backdrop is the ordered dither by default.

## Checking it

Render every state at 1000 and 640 points (and the settings dialog's width for a settings page) and open each PNG. Look
for anything touching, overlapping, squeezed or wrapping one word a line. See `SKILL.md`.
