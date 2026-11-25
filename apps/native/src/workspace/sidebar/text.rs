//! The sidebar's text size: its own setting, apart from the terminals' font size.

use gpui::{App, Global, Pixels, px};

/// The choices on Settings' Text page: the size of a project's name in the sidebar, and the label that shows it.
pub(in crate::workspace) const TEXT_SIZES: [(f32, &str); 3] =
    [(11.0, "Small"), (12.0, "Default"), (13.0, "Large")];
pub(in crate::workspace) const DEFAULT_TEXT: f32 = 12.0;
/// The name size the sidebar's other sizes were drawn for, before the setting: they keep their ratio to it.
const DRAWN_FOR: f32 = 13.0;

/// The size the sidebar's text uses, once set; the default until then.
pub(in crate::workspace) struct SidebarText(pub f32);

impl Global for SidebarText {}

/// A project name's size in the sidebar.
pub(in crate::workspace) fn text_size(cx: &App) -> f32 {
    cx.try_global::<SidebarText>()
        .map_or(DEFAULT_TEXT, |text| text.0)
}

/// What every size in the sidebar is multiplied by, read once where a part of the sidebar is drawn.
#[derive(Clone, Copy)]
pub(in crate::workspace) struct Scale(f32);

impl Scale {
    pub(in crate::workspace) fn of(cx: &App) -> Self {
        Self(text_size(cx) / DRAWN_FOR)
    }

    /// A size drawn for 13px names, in whole or half pixels.
    pub(in crate::workspace) fn px(self, drawn: f32) -> Pixels {
        px((drawn * self.0 * 2.0).round() / 2.0)
    }
}

/// A saved size as the nearest choice, so a hand-edited file can't leave the sidebar unreadable.
pub(in crate::workspace) fn snap_text(size: f32) -> f32 {
    TEXT_SIZES
        .iter()
        .map(|(step, _)| *step)
        .min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs()))
        .unwrap_or(DEFAULT_TEXT)
}
