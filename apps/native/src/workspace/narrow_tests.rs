//! Headless window tests for a narrow main column, where the project bar, a pane's header, the find bar and the project
//! page used to run past their edges (`ui::cutoff` finds what a frame cuts off): what they leave out, and that a frame
//! of them cuts nothing off.

use gpui::{AppContext, TestApp, TestAppWindow, px, size};
use wings_editors::EditorView;

use super::{
    Dir, Workspace,
    context_menu::MenuTarget,
    editor::EditorForm,
    pane_card::{PaneAction::*, header_plan},
    project::{BarPlan, Plugins, bar_plan},
    tests::open_sized,
};
use crate::ui;

fn editor(id: &str, name: &str) -> EditorView {
    EditorView { id: id.into(), name: name.into() }
}

/// What the frame the window draws now cuts off, a line each.
pub(super) fn cut_off(app: &mut TestApp, window: &mut TestAppWindow<Workspace>) -> Vec<String> {
    window.draw();
    let handle = window.handle().into();
    app.update(|cx| {
        cx.update_window(handle, |_, window, cx| ui::cutoff::check(window, cx)).unwrap()
    })
    .iter()
    .map(ToString::to_string)
    .collect()
}

#[test]
fn a_header_keeps_all_its_buttons_until_the_pane_is_narrow_and_then_the_least_needed_go_behind_more() {
    assert_eq!(header_plan(900.0, false), (vec![Close, Zoom, SplitRight, SplitDown], vec![]));
    // Close stays, and the rest wait behind "…", which takes the place of the one that no longer fits.
    assert_eq!(header_plan(160.0, false), (vec![Close], vec![Zoom, SplitRight, SplitDown]));
    // With no room for any, only "…" shows, and everything an agent's terminal has is behind it.
    let (shown, more) = header_plan(110.0, true);
    assert!(shown.is_empty());
    assert_eq!(more, vec![Close, Hide, Zoom, SplitRight, SplitDown, Float, Move]);
}

#[test]
fn the_project_bar_gives_up_its_plugin_buttons_then_the_editors_words_then_the_branch_as_its_column_narrows() {
    let editor = Some((3, true));
    let bar = |width| bar_plan(width, editor, &Plugins { letters: &[7, 7, 6], menu: false });
    assert_eq!(bar(850.0), BarPlan { editor: EditorForm::Full, inline: 3, branch: true, labels: true });
    // The titles go first, with room for the buttons themselves.
    assert_eq!(bar(520.0), BarPlan { editor: EditorForm::Full, inline: 3, branch: true, labels: false });
    // Without room for the words, they go before the plugin buttons do, which are in its menu by then.
    assert_eq!(bar(250.0), BarPlan { editor: EditorForm::Icon, inline: 0, branch: true, labels: false });
    // The column of a 640 window with both sidebars open.
    assert_eq!(bar(150.0), BarPlan { editor: EditorForm::Merged, inline: 0, branch: false, labels: false });
}

#[test]
fn a_narrow_panes_buttons_go_behind_a_more_button_that_opens_its_menu() {
    let (_app, mut window) = open_sized(Some(size(px(1280.0), px(900.0))), |_| {});
    // Each split halves the pane that has the keys: the first pane stays wide, the last is a sliver.
    window.update(|w, window, cx| {
        for _ in 0..5 {
            w.split(Dir::Row, window, cx);
        }
    });
    window.draw();
    assert!(ui::spot_at("pane-more:0").is_none(), "the wide pane has room for its buttons");
    assert!(ui::spot_at("pane-more:2").is_some(), "the narrow pane has a \"…\"");
    super::plugin_tests::click_spot(&mut window, "pane-more:2");
    window.read(|w, _| match w.menu.as_ref().map(|menu| &menu.target) {
        Some(MenuTarget::PaneMore(2, moved)) => assert!(moved.contains(&SplitRight)),
        other => panic!("the menu is {other:?}"),
    });
}

#[test]
fn a_narrow_column_cuts_off_neither_the_project_bar_nor_a_panes_header() {
    let (mut app, mut window) = open_sized(Some(size(px(400.0), px(700.0))), |config| {
        config.editors = Some(vec![editor("vscode", "VS Code"), editor("zed", "Zed")]);
    });
    let cuts = cut_off(&mut app, &mut window);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

#[test]
fn the_find_bar_stays_inside_a_narrow_pane() {
    // The pane is 384 wide, less than the bar's 406 and the 12 points it keeps from each edge.
    let (mut app, mut window) = open_sized(Some(size(px(640.0), px(700.0))), |_| {});
    window.draw();
    window.simulate_keystrokes("cmd-f");
    window.draw();
    let bar = ui::spot_at("find-bar").expect("Cmd-F opens the find bar");
    assert!(bar.left() >= px(0.0), "the bar starts at {:?}", bar.left());
    let cuts = cut_off(&mut app, &mut window);
    assert!(cuts.is_empty(), "{cuts:#?}");
}

#[test]
fn a_narrow_panes_title_keeps_its_room_when_the_folder_after_it_is_long() {
    use crate::agents::PaneInfo;
    let (_app, mut window) = open_sized(Some(size(px(640.0), px(700.0))), |_| {});
    window.update(|w, _, _| {
        w.pane_info.insert(
            0,
            PaneInfo {
                command: "zsh".into(),
                cwd: Some("/private/tmp/app-polish/tmp/wings-native-shot-projects/acme-web".into()),
                busy: false,
            },
        );
    });
    window.draw();
    let title = ui::spot_at("pane-title:0").expect("the header spots its title");
    // "zsh" in the header's 13 point type is about 24 points: the folder shrinks first, not the title down to "…".
    assert!(title.size.width >= px(22.0), "the title is {:?} wide", title.size.width);
    assert!(ui::spot_at("pane-detail:0").is_some(), "the folder shows in a pane with the room");
    // Split, each is about 187 wide: no room for more than "…" of the folder, so it is left out.
    window.update(|w, window, cx| w.split(Dir::Row, window, cx));
    ui::forget_spots();
    window.draw();
    assert!(ui::spot_at("pane-detail:0").is_none(), "the narrow pane leaves the folder out");
}

#[test]
fn a_project_menu_taller_than_the_window_stays_inside_it_and_scrolls() {
    let (_app, mut window) = open_sized(Some(size(px(1000.0), px(300.0))), |_| {});
    let project = window.read(|w, _| w.projects[0].path.clone());
    window.update(|w, _, cx| w.open_menu(MenuTarget::Project(project), gpui::point(px(100.0), px(100.0)), cx));
    window.draw();
    let menu = ui::spot_at("context-menu").expect("the menu is drawn");
    assert!(menu.top() >= px(0.0) && menu.bottom() <= px(300.0), "the menu runs from {:?} to {:?}", menu.top(), menu.bottom());
}
