//! The terminals' background: it follows the theme, a colour of the user's own wins, and the choice is saved.

use super::tests::open_with;
use gpui::TestAppWindow;
use crate::{
    terminal_view::{default_colors, look},
    theme::{self, Theme},
};

fn ground(window: &TestAppWindow<super::Workspace>) -> u32 {
    window.read(|_, cx| default_colors(look(cx).background, theme::surface()).1)
}

#[test]
fn the_ground_follows_the_theme_until_a_colour_is_chosen_and_comes_back_after_a_relaunch() {
    let dir = std::env::temp_dir().join(format!("wings-native-term-bg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("workspace.json");
    {
        let (_app, mut window) = open_with(Some(file.clone()));
        assert_eq!(ground(&window), 0x17151b);
        window.update(|w, _, cx| w.set_appearance(Theme::Nord, true, cx));
        assert_eq!(ground(&window), Theme::Nord.colors().surface, "the theme's");
        window.update(|w, _, cx| w.set_terminal_background(Some(0x1e1e2e), cx));
        assert_eq!(ground(&window), 0x1e1e2e, "the user's colour wins");
        window.update(|w, _, cx| w.set_appearance(Theme::Forest, true, cx));
        assert_eq!(ground(&window), 0x1e1e2e, "and keeps winning when the theme changes");
    }
    {
        let (_app, mut window) = open_with(Some(file.clone()));
        assert_eq!(ground(&window), 0x1e1e2e, "saved");
        window.update(|w, _, cx| w.set_terminal_background(None, cx));
        assert_eq!(ground(&window), Theme::Forest.colors().surface, "follows the theme again");
    }
    let (_app, window) = open_with(Some(file));
    assert_eq!(window.read(|_, cx| look(cx).background), None, "the choice to follow is saved too");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_typed_colour_that_is_not_rrggbb_is_refused_with_a_note() {
    let (_app, mut window) = open_with(None);
    window.update(|w, window, cx| w.open_settings(window, cx));
    let type_in = |window: &mut TestAppWindow<super::Workspace>, text: &str| {
        window.update(|w, _, cx| {
            let field = w.settings.as_ref().unwrap().background.clone();
            field.update(cx, |field, _| field.set_text(text.into()));
            w.apply_background_text(cx);
        });
    };
    type_in(&mut window, "1e1e2e");
    assert_eq!(window.read(|_, cx| look(cx).background), None);
    assert!(window.read(|w, _| w.settings.as_ref().unwrap().background_error.is_some()));
    type_in(&mut window, "#1E1E2E");
    assert_eq!(window.read(|_, cx| look(cx).background), Some(0x1e1e2e));
    assert!(window.read(|w, _| w.settings.as_ref().unwrap().background_error.is_none()));
}
