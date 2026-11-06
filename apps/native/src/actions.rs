//! The app's commands, with the desktop app's keys (`src/lib/commands.json`) and menus
//! (`src-tauri/src/menu.rs`). App shortcuts are Cmd on macOS and Ctrl-Shift elsewhere, where plain Ctrl
//! belongs to the shell.

use gpui::{Action, App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

actions!(
    wings,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        CloseWindow,
        Minimize,
        Zoom,
        ToggleFullScreen,
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        MoveTabLeft,
        MoveTabRight,
        SplitRight,
        SplitDown,
        ZoomPane,
        ClosePane,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        Find,
        Copy,
        Paste,
        ToggleSidebar,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        OpenSettings,
        TogglePalette,
        OpenHistory,
        OpenProjectInEditor,
        ToggleQuickTerminal,
        PaletteNext,
        PalettePrevious,
        PaletteFirst,
        PaletteLast,
    ]
);

/// Cmd-1 to Cmd-9: the tab at that position.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = wings, no_json)]
pub struct ActivateTab(pub usize);

const MAC: bool = cfg!(target_os = "macos");

/// The binding for this platform: `mac` on macOS, `other` elsewhere.
fn key<A: Action>(mac: &'static str, other: &'static str, action: A) -> KeyBinding {
    KeyBinding::new(if MAC { mac } else { other }, action, None)
}

/// Inside a terminal, where the find field handles Paste and Find for itself.
fn terminal_key<A: Action>(mac: &'static str, other: &'static str, action: A) -> KeyBinding {
    KeyBinding::new(if MAC { mac } else { other }, action, Some("Terminal"))
}

/// Binds the keys and the app-wide commands. The window's commands are handled by the workspace.
pub fn bind(cx: &mut App) {
    let digits = [
        ("cmd-1", "ctrl-shift-1"),
        ("cmd-2", "ctrl-shift-2"),
        ("cmd-3", "ctrl-shift-3"),
        ("cmd-4", "ctrl-shift-4"),
        ("cmd-5", "ctrl-shift-5"),
        ("cmd-6", "ctrl-shift-6"),
        ("cmd-7", "ctrl-shift-7"),
        ("cmd-8", "ctrl-shift-8"),
        ("cmd-9", "ctrl-shift-9"),
    ];
    let mut bindings = vec![
        key("cmd-q", "ctrl-shift-q", Quit),
        key("cmd-t", "ctrl-shift-t", NewTab),
        key("cmd-]", "ctrl-shift-]", NextTab),
        key("cmd-[", "ctrl-shift-[", PreviousTab),
        key("cmd-d", "ctrl-shift-d", SplitRight),
        key("cmd-shift-d", "ctrl-shift-alt-d", SplitDown),
        key("cmd-w", "ctrl-shift-w", ClosePane),
        key("cmd-alt-left", "ctrl-shift-alt-left", FocusLeft),
        key("cmd-alt-right", "ctrl-shift-alt-right", FocusRight),
        key("cmd-alt-up", "ctrl-shift-alt-up", FocusUp),
        key("cmd-alt-down", "ctrl-shift-alt-down", FocusDown),
        terminal_key("cmd-f", "ctrl-shift-f", Find),
        terminal_key("cmd-c", "ctrl-shift-c", Copy),
        terminal_key("cmd-v", "ctrl-shift-v", Paste),
        // The desktop's "+" is the = key, with or without Shift. The first binding is the one the palette shows.
        key("cmd-+", "ctrl-shift-+", IncreaseFontSize),
        key("cmd-=", "ctrl-shift-=", IncreaseFontSize),
        key("cmd--", "ctrl-shift--", DecreaseFontSize),
        key("cmd-0", "ctrl-shift-0", ResetFontSize),
        key("cmd-b", "ctrl-shift-b", ToggleSidebar),
        // Plain Ctrl elsewhere, as in the desktop app: no shell uses Ctrl-comma.
        key("cmd-,", "ctrl-,", OpenSettings),
        key("cmd-\\", "ctrl-shift-\\", ToggleSidebar),
        key("cmd-k", "ctrl-shift-k", TogglePalette),
        // The desktop's default, Ctrl+Backquote, on every platform.
        KeyBinding::new("ctrl-`", ToggleQuickTerminal, None),
        // The palette's list, which the arrow keys would otherwise move the search field's caret in.
        KeyBinding::new("down", PaletteNext, Some("Palette")),
        KeyBinding::new("ctrl-n", PaletteNext, Some("Palette")),
        KeyBinding::new("up", PalettePrevious, Some("Palette")),
        KeyBinding::new("ctrl-p", PalettePrevious, Some("Palette")),
        KeyBinding::new("home", PaletteFirst, Some("Palette")),
        KeyBinding::new("end", PaletteLast, Some("Palette")),
    ];
    bindings.extend(
        digits
            .into_iter()
            .enumerate()
            .map(|(index, (mac, other))| key(mac, other, ActivateTab(index))),
    );
    if MAC {
        bindings.extend([
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-shift-w", CloseWindow, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-ctrl-f", ToggleFullScreen, None),
        ]);
    }
    cx.bind_keys(bindings);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
}

/// The desktop app's menus, less what the native app doesn't have yet.
pub fn menus() -> Vec<Menu> {
    let mut menus = Vec::new();
    if MAC {
        menus.push(Menu::new("Wings Native").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Wings Native", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Wings Native", Quit),
        ]));
    }
    let mut file = vec![MenuItem::action("Close Pane", ClosePane)];
    if MAC {
        file.push(MenuItem::action("Close Window", CloseWindow));
    } else {
        file.extend([
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ]);
    }
    menus.push(Menu::new("File").items(file));
    menus.push(Menu::new("Edit").items([
        MenuItem::action("Copy", Copy),
        MenuItem::action("Paste", Paste),
    ]));
    menus.push(Menu::new("Tab").items([
        MenuItem::action("New Tab", NewTab),
        MenuItem::action("Close Tab", CloseTab),
        MenuItem::action("Next Tab", NextTab),
        MenuItem::action("Previous Tab", PreviousTab),
        MenuItem::action("Move Tab Left", MoveTabLeft),
        MenuItem::action("Move Tab Right", MoveTabRight),
    ]));
    menus.push(Menu::new("Pane").items([
        MenuItem::action("Split Right", SplitRight),
        MenuItem::action("Split Down", SplitDown),
        MenuItem::action("Zoom Pane", ZoomPane),
        MenuItem::separator(),
        MenuItem::action("Focus Pane Left", FocusLeft),
        MenuItem::action("Focus Pane Right", FocusRight),
        MenuItem::action("Focus Pane Above", FocusUp),
        MenuItem::action("Focus Pane Below", FocusDown),
    ]));
    let mut view = vec![
        MenuItem::action("Toggle Sidebar", ToggleSidebar),
        MenuItem::action("Find in Pane", Find),
        MenuItem::action("Command Palette", TogglePalette),
        MenuItem::action("Past Sessions", OpenHistory),
        MenuItem::action("Quick Terminal", ToggleQuickTerminal),
        MenuItem::separator(),
        MenuItem::action("Increase Font Size", IncreaseFontSize),
        MenuItem::action("Decrease Font Size", DecreaseFontSize),
        MenuItem::action("Reset Font Size", ResetFontSize),
    ];
    if MAC {
        view.extend([
            MenuItem::separator(),
            MenuItem::action("Enter Full Screen", ToggleFullScreen),
        ]);
    }
    menus.push(Menu::new("View").items(view));
    menus.push(Menu::new("Window").items([
        MenuItem::action("Minimize", Minimize),
        MenuItem::action("Zoom", Zoom),
    ]));
    menus
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use gpui::TestApp;

    /// The desktop shows a shortcut beside every menu item that has one, so the native menus must too.
    #[test]
    fn every_menu_command_with_a_desktop_shortcut_has_its_key() {
        let mut app = TestApp::new();
        app.update(bind);
        let unbound: [&dyn Action; 8] = [
            &CloseTab,
            &OpenHistory,
            &MoveTabLeft,
            &MoveTabRight,
            &ZoomPane,
            &ShowAll,
            &Zoom,
            &CloseWindow,
        ];
        app.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            for menu in menus() {
                for item in &menu.items {
                    let MenuItem::Action { name, action, .. } = item else {
                        continue;
                    };
                    let bound = keymap.bindings_for_action(action.as_ref()).next().is_some();
                    let expected = !unbound.iter().any(|none| none.partial_eq(action.as_ref()))
                        || (MAC && action.partial_eq(&CloseWindow));
                    assert_eq!(bound, expected, "{} > {name}", menu.name);
                }
            }
        });
    }
}
