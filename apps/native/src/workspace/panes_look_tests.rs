//! The redesign's panes and project bar: panes flush with one shared line, and the bar's plugin buttons with their titles.

use gpui::{px, size};

use super::{
    Dir,
    editor::EditorForm,
    plugin_ui::count_of,
    project::{BarPlan, Plugins, bar_plan},
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
fn a_tab_page_runs_flush_under_the_project_bars_line_as_panes_do() {
    let (_app, mut window) = open_sized(Some(size(px(1000.0), px(700.0))), |_| {});
    window.draw();
    let panes = window.read(|w, _| w.area.expect("the pane area is measured"));
    window.update(|w, window, cx| {
        w.issues_open = true;
        w.show(super::Active::Issues, window, cx);
    });
    window.draw();
    let card = ui::spot_at("page-card").expect("the tab's page");
    // An inset, rounded card left the project bar's line running past its corner.
    assert_eq!((card.left(), card.top(), card.right()), (panes.left(), panes.top(), panes.right()), "where the panes are");
}

#[test]
fn a_wide_project_bar_shows_plugin_titles_and_a_narrow_one_keeps_only_their_icons() {
    let editor = Some((3, true));
    let wide = bar_plan(900.0, editor, &Plugins { letters: &[7, 7], menu: false });
    assert_eq!(wide, BarPlan { editor: EditorForm::Full, inline: 2, branch: true, labels: true });
    // Room for everything but the titles: they go first, before any button leaves the bar.
    let narrower = bar_plan(450.0, editor, &Plugins { letters: &[7, 7], menu: false });
    assert_eq!(narrower, BarPlan { editor: EditorForm::Full, inline: 2, branch: true, labels: false });
}

#[test]
fn only_a_label_of_digits_is_a_count() {
    assert_eq!(count_of(" 12 "), Some("12"));
    assert_eq!(count_of("12:03"), None);
    assert_eq!(count_of(""), None);
}

/// `count` plugins `p0`, `p1`, ... with a sidebar `s` each, titled `<title><n>`.
fn many_plugin(count: usize, title: &str) -> crate::installed::fixture::Data {
    use crate::page::runtime::fake::idle;
    let data = crate::installed::fixture::Data::temp();
    for n in 0..count {
        let sidebar = serde_json::json!({ "id": "s", "title": format!("{title}{n}"), "page": "dist/s.html" });
        data.install_script(
            &format!("p{n}"),
            serde_json::json!({ "contributes": { "sidebars": [sidebar] } }),
            &idle("main"),
            &[("s", &idle("page"))],
            &format!("p{n}-scope"),
        );
    }
    data
}

fn open_many(
    data: &crate::installed::fixture::Data,
    width: f32,
    state: Option<std::path::PathBuf>,
) -> (gpui::TestApp, gpui::TestAppWindow<super::Workspace>) {
    let (app, mut window) = open_sized(Some(size(px(width), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        config.state = state;
    });
    let system: std::sync::Arc<dyn super::plugin_env::System> = std::sync::Arc::<super::plugin_tests::FakeSystem>::default();
    window.read(|w, _| w.plugin_env.use_system(system));
    window.draw();
    (app, window)
}

fn pin(window: &mut gpui::TestAppWindow<super::Workspace>, count: usize) {
    for n in 0..count {
        window.update(|w, _, cx| w.toggle_pin(&format!("p{n}:s"), cx));
    }
    window.draw();
}

#[test]
fn every_pinned_plugin_button_shows_in_the_bar_and_stays_after_a_relaunch() {
    let data = many_plugin(7, "Plug");
    let state = std::env::temp_dir().join(format!("wings-native-pinbar-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&state);
    let (_app, mut window) = open_many(&data, 1600.0, Some(state.clone()));
    pin(&mut window, 5);
    for n in 0..5 {
        assert!(ui::spot_at(&format!("bar-button:p{n}:s")).is_some(), "pinned button {n} is in the bar");
    }
    assert!(ui::spot_at("bar-button:p6:s").is_none(), "an unpinned one is in the menu");
    drop(window);
    ui::forget_spots();
    let (_app, mut window) = open_many(&data, 1600.0, Some(state.clone()));
    window.draw();
    for n in 0..5 {
        assert!(ui::spot_at(&format!("bar-button:p{n}:s")).is_some(), "pinned button {n} is back after a relaunch");
    }
    let _ = std::fs::remove_file(state);
}

#[test]
fn pinned_titles_drop_to_icons_when_they_pass_half_the_bar_or_do_not_fit() {
    // Two short titles fit in half the bar of a wide window: icon and title.
    let data = many_plugin(7, "Plug");
    let (_app, mut window) = open_many(&data, 1600.0, None);
    pin(&mut window, 2);
    let titled = ui::spot_at("bar-button:p0:s").expect("a pinned button").size.width;
    assert!(titled > px(60.0), "icon and title, {titled:?}");
    // Seven long titles would take more than half the bar: all of them show as icons.
    let data = many_plugin(7, "A long plugin title ");
    let (_app, mut window) = open_many(&data, 1600.0, None);
    pin(&mut window, 7);
    for n in 0..7 {
        let button = ui::spot_at(&format!("bar-button:p{n}:s")).unwrap_or_else(|| panic!("button {n} is in the bar"));
        assert!(button.size.width < px(40.0), "button {n} is an icon, {:?}", button.size.width);
    }
    // In a narrow window a few titled buttons don't fit: icons, and still all of them.
    let (_app, mut window) = open_many(&many_plugin(7, "Plug"), 1000.0, None);
    pin(&mut window, 4);
    let narrow = ui::spot_at("bar-button:p3:s").expect("the fourth pinned button");
    assert!(narrow.size.width < px(40.0), "an icon, {:?}", narrow.size.width);
}

#[test]
fn the_right_panels_header_ends_where_the_project_bar_does() {
    let (_app, mut window) = open_many(&many_plugin(2, "Plug"), 1000.0, None);
    window.update(|w, _, cx| assert!(w.open_sidebar_from_plugin("p0", "s", cx)));
    window.draw();
    window.draw();
    let bar = ui::spot_at("project-bar").expect("the project bar");
    let header = ui::spot_at("right-header").expect("the right panel's header");
    assert_eq!(header.bottom(), bar.bottom(), "their lines meet");
}
