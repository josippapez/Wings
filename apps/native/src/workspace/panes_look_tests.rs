//! The redesign's panes and project bar: panes flush with one shared line, and the bar's plugin buttons with their titles.

use gpui::{px, size};

use super::{
    Dir,
    editor::EditorForm,
    plugin_ui::count_of,
    project::{BarPlan, bar_plan},
    tests::open_sized,
};
use crate::{layout::SEAM, ui};

#[test]
fn two_panes_share_one_line_with_no_gutter() {
    let (_app, mut window) = open_sized(Some(size(px(640.0), px(700.0))), |_| {});
    window.update(|w, window, cx| w.split(Dir::Row, window, cx));
    window.draw();
    let area = window.read(|w, _| w.area.expect("the pane area is measured"));
    let focused = window.read(|w, _| w.terminals[0].focused);
    let rule = ui::spot_at(&format!("pane-header:{focused}")).expect("the new pane's header");
    let other = if focused == 0 { 1 } else { 0 };
    // The new pane is the second: it runs flush to the area's edge, and the first ends one line before it.
    assert_eq!(rule.right(), area.right(), "no gutter at the area's edge");
    let first = ui::spot_at(&format!("pane-title:{other}")).expect("the first pane's title");
    assert!(first.left() >= area.left() && first.right() < rule.left());
    assert_eq!(rule.left(), area.left() + area.size.width * 0.5 + px(SEAM), "one point between the panes");
}

#[test]
fn a_wide_project_bar_shows_plugin_titles_and_a_narrow_one_keeps_only_their_icons() {
    let editor = Some((3, true));
    let wide = bar_plan(900.0, editor, 2, 14);
    assert_eq!(wide, BarPlan { editor: EditorForm::Full, inline: 2, branch: true, labels: true });
    // Room for everything but the titles: they go first, before any button leaves the bar.
    let narrower = bar_plan(450.0, editor, 2, 14);
    assert_eq!(narrower, BarPlan { editor: EditorForm::Full, inline: 2, branch: true, labels: false });
}

#[test]
fn only_a_label_of_digits_is_a_count() {
    assert_eq!(count_of(" 12 "), Some("12"));
    assert_eq!(count_of("12:03"), None);
    assert_eq!(count_of(""), None);
}
