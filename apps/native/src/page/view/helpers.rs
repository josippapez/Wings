//! The page view's free helpers: finding nodes in a tree, text measuring, tones and sizes.
use super::*;

/// Whether the page shows a sheet, wherever in its tree.
pub(super) fn sheet_in(nodes: &[Node]) -> bool {
    nodes
        .iter()
        .any(|node| matches!(node, Node::Sheet(_)) || node.parts().into_iter().any(sheet_in))
}

/// The dialog the page shows, wherever in its tree it put it.
pub(super) fn dialog_in(nodes: &[Node]) -> Option<&tree::Dialog> {
    nodes.iter().rev().find_map(|node| match node {
        Node::Dialog(dialog) => Some(dialog),
        other => other.parts().into_iter().rev().find_map(dialog_in),
    })
}

/// The click handler of the button in `nodes` whose `shortcut` is `keystroke`.
pub(super) fn shortcut_in(nodes: &[Node], keystroke: &gpui::Keystroke) -> Option<String> {
    let mods = keystroke.modifiers;
    nodes.iter().find_map(|node| {
        let button = match node {
            Node::Button(button) => Some(button),
            Node::ListButton(item) => Some(&item.button),
            _ => None,
        };
        button
            .filter(|button| {
                button.shortcut.as_ref().is_some_and(|key| {
                    (key.cmd, key.ctrl, key.alt, key.shift) == (mods.platform, mods.control, mods.alt, mods.shift)
                        && key.key == keystroke.key
                })
            })
            .and_then(|button| button.on_click.clone())
            .or_else(|| node.parts().into_iter().find_map(|nodes| shortcut_in(nodes, keystroke)))
    })
}

/// The `onTextScale` of the page's `Page`, wherever in its tree it is.
pub(super) fn text_scale_in(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Page(page) if page.on_text_scale.is_some() => page.on_text_scale.clone(),
        other => other.parts().into_iter().find_map(text_scale_in),
    })
}

/// The popup menu the page shows, wherever in its tree it put it.
pub(super) fn popup_in(nodes: &[Node]) -> Option<&tree::PopupMenu> {
    nodes.iter().rev().find_map(|node| match node {
        Node::PopupMenu(menu) => Some(menu),
        other => other.parts().into_iter().rev().find_map(popup_in),
    })
}

/// Milliseconds since 1970, as a `since` counts from.
pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |now| now.as_millis() as i64)
}

/// How long ago `since` was, at `now` (both ms since the epoch), as a status line counts: "16s", "2m 5s", "1h 4m".
pub(super) fn took(since: i64, now: i64) -> String {
    let seconds = (now - since).max(0) / 1000;
    match seconds {
        ..60 => format!("{seconds}s"),
        ..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// How long after a click or key in a page it may still move the keyboard into itself, as a dialog it opened does.
pub(super) const GESTURE_TAKES_KEYBOARD: Duration = Duration::from_secs(2);

/// How near its end, in points, a following `Scroll` still counts as there: a line's worth of a pointer's drift.
pub(super) const FOLLOW_SLACK: f32 = 24.0;
/// How tall an opened `expandable` text grows before it scrolls inside its own box.
pub(super) const OPEN_TEXT_HEIGHT: f32 = 240.0;
pub(super) const PAGE_CONTEXT: &str = "plugin-page";
/// The group a list item with actions names itself, for its actions to show while it is pointed at.
pub(super) const LIST_ITEM_GROUP: &str = "page-list-item";

/// Only shortcuts used by Issues cross the page boundary. Ordinary editing/navigation stays with the native control.
pub(super) fn send_shortcut(
    view: &PageView,
    handler: &str,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<PageView>,
) -> bool {
    let key = match event.keystroke.key.as_str() {
        "left" if event.keystroke.modifiers.alt => "ArrowLeft",
        "right" if event.keystroke.modifiers.alt => "ArrowRight",
        "delete" | "backspace" => "Delete",
        // An issue's title edits on F2, as a name does in Finder.
        "f2" => "F2",
        _ => return false,
    };
    let modifiers = event.keystroke.modifiers;
    view.act(handler, Some(&serde_json::json!({"type":"keydown", "key":key, "altKey":modifiers.alt, "metaKey":modifiers.platform, "ctrlKey":modifiers.control, "shiftKey":modifiers.shift})));
    window.prevent_default();
    cx.stop_propagation();
    true
}

/// How far a floating split `width` wide sets its start in for its pane `pane` wide: the pane's width, or with `column`, only
/// as far as keeps a centred column that wide clear of the pane, in 8-point steps so a drag of the window draws the view
/// a few dozen times rather than at every point.
pub(super) fn inset_by(width: f32, pane: f32, column: Option<f32>) -> f32 {
    let pane = pane.min(width / 2.0);
    match column {
        None => pane,
        Some(column) => (((column + 2.0 * pane - width).max(0.0) / 8.0).ceil() * 8.0).min(pane),
    }
}

pub(super) fn gap(gap: Gap) -> f32 {
    match gap {
        Gap::None => 0.0,
        Gap::Xs => 2.0,
        Gap::S => 4.0,
        Gap::M => 8.0,
        Gap::L => 12.0,
    }
}

/// A `clamp`ed text: three lines, the last ending in an ellipsis where the text goes on, as `-webkit-line-clamp` ends
/// it. Without the ellipsis the third line ran on to the box's edge and was cut there, mid-word ("keep the tota").
pub(in super::super) fn three_lines<E: Styled>(el: E) -> E {
    el.line_clamp(3).text_ellipsis()
}

/// The font a page's text is drawn in.
pub(super) fn page_font(weight: FontWeight, mono: bool, cx: &gpui::App) -> gpui::FontId {
    let family = if mono {
        DEFAULT_FONT_FAMILY
    } else {
        ".SystemUIFont"
    };
    cx.text_system().resolve_font(&gpui::Font {
        weight,
        ..gpui::font(family)
    })
}

/// How far down each text of a baseline row of texts starts its first line, so that their baselines meet, as CSS lines
/// them up; `None` for any other row. Their lines are the tallest's, but for the sizes that set their own, and GPUI sets
/// a line's text in its middle with the baseline `(ascent - descent) / 2` under it, so a smaller text's baseline sat
/// higher than a larger one's: an issue's key three points over its title's. A `wrap` button of one text is that text,
/// inside the button's border and padding.
pub(in super::super) fn baseline_drops(row: &tree::Row, cx: &gpui::App) -> Option<Vec<f32>> {
    let line = line_height(&row.children).filter(|_| row.align == Align::Baseline)?;
    let system = cx.text_system();
    let baselines: Vec<f32> = row
        .children
        .iter()
        .map(|node| {
            let Some((text, top)) = row_text(node) else {
                return 0.0;
            };
            let (size, line) = match text.size {
                TextSize::Normal => (scaled(13.0), line),
                TextSize::Small => (scaled(12.0), line),
                TextSize::Tiny => (scaled(11.0), line),
                TextSize::Large => (scaled(20.0), scaled(26.0)),
                TextSize::Field => (scaled(14.0), scaled(21.0)),
            };
            let weight = match text.weight {
                Weight::Normal => FontWeight::NORMAL,
                Weight::Medium => FontWeight::MEDIUM,
                Weight::Strong => FontWeight::SEMIBOLD,
            };
            let font = page_font(weight, text.mono, cx);
            // A font's descent is under its baseline, so GPUI gives it as a negative length.
            let (ascent, descent) = (
                system.ascent(font, px(size)),
                system.descent(font, px(size)),
            );
            top + line / 2.0 + (ascent + descent).as_f32() / 2.0
        })
        .collect();
    let lowest = baselines.iter().copied().fold(0.0, f32::max);
    Some(
        baselines
            .into_iter()
            .map(|baseline| lowest - baseline)
            .collect(),
    )
}

/// The height of the tallest line of `nodes`, when they are all texts (`row_text`): a text's lines are 1.45 of its size,
/// the page's line height, but for the sizes that set their own.
pub(super) fn line_height(nodes: &[Node]) -> Option<f32> {
    nodes.iter().try_fold(None::<f32>, |tallest, node| {
        let (text, _) = row_text(node)?;
        let height = match text.size {
            TextSize::Normal => scaled(13.0 * 1.45),
            TextSize::Small => scaled(12.0 * 1.45),
            TextSize::Tiny => scaled(11.0 * 1.45),
            TextSize::Large => scaled(26.0),
            TextSize::Field => scaled(14.0 * 1.5),
        };
        Some(Some(tallest.map_or(height, |tallest| tallest.max(height))))
    })?
}

/// The padding over and under a `wrap` button's text, as plugin-ui.css's `.w-button[data-wrap]`.
pub(super) const WRAPPED_PADDING: f32 = 2.0;

/// The text a baseline row lines `node` up by, and how far into `node` its top is: a text's own, or the one text of a
/// `wrap` button or of a list button, under the button's 1pt border and its padding. A log's folded group is its
/// number beside a list button that holds its title.
pub(super) fn row_text(node: &Node) -> Option<(&tree::Text, f32)> {
    match node {
        Node::Text(text) => Some((text, 0.0)),
        Node::Button(button) if button.wrap => match button.children.as_slice() {
            [Node::Text(text)] => Some((text, 1.0 + WRAPPED_PADDING)),
            _ => None,
        },
        Node::ListButton(item) if !item.vertical => match item.button.children.as_slice() {
            [Node::Text(text)] => Some((text, 1.0 + controls_view::LIST_BUTTON_PADDING)),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn tone_color(tone: Tone) -> u32 {
    match tone {
        Tone::Muted => theme::palette().muted_foreground,
        Tone::Info => theme::palette().info,
        Tone::Success => theme::palette().done,
        Tone::Warn => theme::palette().working,
        Tone::Danger => theme::palette().danger,
    }
}

/// A gauge's bar is `--wings-info` until it reaches a level.
pub(in super::super) fn level_color(level: Level) -> u32 {
    match level {
        Level::Normal => theme::palette().info,
        Level::Warn => tone_color(Tone::Warn),
        Level::Danger => tone_color(Tone::Danger),
    }
}

pub(super) fn variant(variant: tree::Variant) -> ui::Variant {
    match variant {
        tree::Variant::Default => ui::Variant::Default,
        tree::Variant::Secondary => ui::Variant::Secondary,
        tree::Variant::Outline => ui::Variant::Outline,
        tree::Variant::Ghost => ui::Variant::Ghost,
        tree::Variant::Destructive => ui::Variant::Destructive,
        tree::Variant::Link => ui::Variant::Link,
    }
}

/// The most a text holds out for its longest word. A word wider than this, like a long path in a chat reply, breaks
/// inside itself as a browser's `overflow-wrap: anywhere` does: holding out for all of it pushed the line past the page's
/// edge.
pub(super) const LONGEST_WORD_KEPT: f32 = 240.0;

/// The width of the widest word of `text` at `size`, in the page's font or the terminal's with `mono`, up to
/// `LONGEST_WORD_KEPT`.
/// A text's size and weight, as `text_element` sets them.
pub(super) fn text_font(text: &tree::Text) -> (f32, FontWeight) {
    let size = match text.size {
        TextSize::Small => scaled(12.0),
        TextSize::Tiny => scaled(11.0),
        TextSize::Large => scaled(20.0),
        TextSize::Field => scaled(14.0),
        TextSize::Normal => scaled(13.0),
    };
    let weight = match text.weight {
        Weight::Normal => FontWeight::NORMAL,
        Weight::Medium => FontWeight::MEDIUM,
        Weight::Strong => FontWeight::SEMIBOLD,
    };
    (size, weight)
}

/// How wide `c` is in `font` at `size`. The text system asks Core Text each time, for the glyph and then its advance; a
/// long chat's page measured the words of its growing texts that way, every character on every render: about a tenth of
/// the page's draw in a profile of the app while typing there. A character's width is its width at one point times the
/// size, so each is asked once.
pub(in super::super) fn char_width(font: gpui::FontId, size: f32, c: char, cx: &gpui::App) -> f32 {
    let unit = CHAR_WIDTHS.with_borrow_mut(|widths| {
        *widths.entry((font, c)).or_insert_with(|| {
            #[cfg(feature = "test-support")]
            CHARS_ASKED.with(|asked| asked.set(asked.get() + 1));
            cx.text_system().advance(font, px(1.0), c).map_or(0.0, |advance| advance.width.as_f32())
        })
    });
    unit * size
}

/// How many characters the page's text measuring has asked the text system for on this thread.
#[cfg(all(test, feature = "test-support"))]
pub(in super::super) fn chars_asked() -> usize {
    CHARS_ASKED.with(Cell::get)
}

/// How wide `text` is on one line.
pub(super) fn line_width(text: &str, size: f32, weight: FontWeight, mono: bool, cx: &gpui::App) -> f32 {
    let font = page_font(weight, mono, cx);
    text.chars().map(|c| char_width(font, size, c, cx)).sum()
}

pub(super) fn longest_word(text: &str, size: f32, weight: FontWeight, mono: bool, cx: &gpui::App) -> f32 {
    let font = page_font(weight, mono, cx);
    let width = |word: &str| word.chars().map(|c| char_width(font, size, c, cx)).sum::<f32>();
    // A point over, so rounding in the line's own layout never leaves the word a hair too wide to fit.
    (text.split_whitespace().map(width).fold(0.0, f32::max).ceil() + 1.0).min(LONGEST_WORD_KEPT)
}

/// A button at its `size`, as plugin-ui.css's `.w-button[data-size]`: `xs` and `sm` are smaller with smaller text, the
/// `icon` sizes square.
pub(super) fn sized(button: gpui::Stateful<gpui::Div>, size: tree::ButtonSize) -> gpui::Stateful<gpui::Div> {
    use tree::ButtonSize as Size;
    match size {
        Size::Default => button,
        Size::Xs => button
            .h(px(24.0))
            .gap(px(4.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .text_size(px(12.0)),
        Size::Sm => button
            .h(px(28.0))
            .gap(px(4.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .text_size(px(12.8)),
        Size::Lg => button.h(px(36.0)),
        Size::Icon => button.w(px(32.0)).px_0(),
        Size::IconXs => button.size(px(24.0)).px_0().rounded(px(8.0)),
        Size::IconSm => button.size(px(28.0)).px_0().rounded(px(8.0)),
        Size::IconLg => button.size(px(36.0)).px_0(),
    }
}

/// How tall a small button is.
pub(super) const SM_BUTTON_HEIGHT: f32 = 28.0;
