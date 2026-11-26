//! The viewer's free helpers: empty-file notes, line colours, numbers and toggles.
use super::*;

/// Why a file shows no lines.
pub(super) fn no_lines(file: &super::super::patch::DiffFile) -> &'static str {
    if file.is_binary {
        "Binary file changed. No text diff."
    } else if file.is_placeholder {
        "This file is too large to show."
    } else {
        match file.status {
            DiffStatus::Added => "Empty file added.",
            DiffStatus::Deleted => "Empty file deleted.",
            DiffStatus::Renamed => "File renamed. No changed text lines.",
            DiffStatus::Copied => "File copied. No changed text lines.",
            DiffStatus::Modified => "File metadata changed. No changed text lines.",
        }
    }
}

/// A line's mark, the mark's colour and the line's background.
pub(super) fn colors(kind: DiffLineKind) -> (&'static str, u32, u32) {
    match kind {
        DiffLineKind::Added => ("+", theme::done(), 0x7dd3a712),
        DiffLineKind::Deleted => ("-", theme::danger(), 0xf2786a12),
        DiffLineKind::Context => (" ", theme::foreground(), 0),
    }
}

/// The desktop's message icon and a number of comments, in the file list and the header. The header's longer text gives
/// way first when the window is narrow.
pub(super) fn comment_count(count: impl std::fmt::Display, size: f32) -> Div {
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(if size < 14.0 { 2.0 } else { 4.0 }))
        .text_color(rgb(theme::muted_foreground()))
        .child(
            div()
                .flex_none()
                .child(icon(Icon::MessageSquare, size, theme::muted_foreground())),
        )
        .child(div().min_w_0().truncate().child(count.to_string()))
}

/// A line number in the gutter.
pub(super) fn number(line: Option<usize>) -> Div {
    div()
        .w(px(48.0))
        .flex_none()
        .text_right()
        .pr(px(8.0))
        .text_color(rgb(theme::muted_foreground()))
        .child(line.map(|n| n.to_string()).unwrap_or_default())
}

/// The desktop's `Toggle` and `ToggleGroupItem` at size `sm`: an icon, with a label when there's room, on a fill while
/// pressed.
pub(super) fn toggle(
    id: impl Into<ElementId>,
    which: Icon,
    label: &'static str,
    text: Option<&'static str>,
    pressed: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(SharedString::from(label))
        .aria_toggled(if pressed {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .h(px(28.0))
        .min_w(px(28.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .px(px(if text.is_some() { 8.0 } else { 6.0 }))
        .rounded(px(6.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .bg(rgba(if pressed { 0xffffff17 } else { 0 }))
        .when(!pressed, |item| {
            item.hover(|style| style.bg(rgba(theme::hover())))
        })
        .child(icon(which, 16.0, theme::foreground()))
        .children(text)
}

/// The desktop's `ToggleGroup`: its items on a faint fill.
pub(super) fn toggle_group(items: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff0d))
        .children(items)
}
