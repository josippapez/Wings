//! The app menu. Same as Tauri's default except for closing: in a terminal app the close
//! shortcut should close the focused pane, not the whole window with every terminal in it.
//! The Tab, Pane and View menus are built from `src/lib/commands.json`, the list the window's shortcuts and the
//! command palette use too; choosing one sends its id to the window, which runs it.

use serde::Deserialize;
use tauri::{
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, Submenu, SubmenuBuilder},
    AppHandle, Emitter, Manager, Runtime,
};

/// One command of `commands.json`. Only what the menu needs.
#[derive(Deserialize)]
struct Command {
    id: String,
    title: String,
    menu: Option<String>,
    #[serde(default)]
    gap: bool,
    key: Option<String>,
    extra: Option<bool>,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    plain: bool,
}

#[derive(Deserialize)]
struct Commands {
    commands: Vec<Command>,
}

fn commands() -> Vec<Command> {
    serde_json::from_str::<Commands>(include_str!("../../src/lib/commands.json")).expect("commands.json").commands
}

/// The menu accelerator for a command's shortcut: ⌘ on macOS, Ctrl+Shift elsewhere, where Alt stands for the
/// window's "extra" key. Matches `shortcutText` in `src/lib/commands.ts`. A command that takes the extra key either
/// way is written without it, since a menu key can't match both.
fn accelerator(c: &Command, mac: bool) -> Option<String> {
    let key = c.key.as_deref()?;
    let key = if key == "+" { "Equal" } else { key };
    let mut parts: Vec<&str> = Vec::new();
    if c.plain {
        parts.push("CmdOrCtrl");
    } else if mac {
        parts.push("Cmd");
        if c.extra == Some(true) {
            parts.push("Shift");
        }
        if c.alt {
            parts.push("Alt");
        }
    } else {
        parts.push("Ctrl");
        parts.push("Shift");
        if c.alt || c.extra == Some(true) {
            parts.push("Alt");
        }
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// A menu of the commands that name it, in the order the list has them.
fn command_menu<'a, R: Runtime>(app: &'a AppHandle<R>, name: &str, commands: &[Command]) -> tauri::Result<SubmenuBuilder<'a, R, AppHandle<R>>> {
    let mut menu = SubmenuBuilder::new(app, name);
    for c in commands.iter().filter(|c| c.menu.as_deref() == Some(name)) {
        let mut item = MenuItemBuilder::with_id(format!("cmd:{}", c.id), &c.title);
        if let Some(accelerator) = accelerator(c, cfg!(target_os = "macos")) {
            item = item.accelerator(accelerator);
        }
        if c.gap {
            menu = menu.separator();
        }
        menu = menu.item(&item.build(app)?);
    }
    Ok(menu)
}

// Plain Ctrl+W deletes a word in the shell, so other platforms use Ctrl+Shift+W like Windows Terminal.
#[cfg(target_os = "macos")]
const CLOSE_PANE: &str = "Cmd+W";
#[cfg(not(target_os = "macos"))]
const CLOSE_PANE: &str = "Ctrl+Shift+W";

fn view_menu<R: Runtime>(app: &AppHandle<R>, commands: &[Command]) -> tauri::Result<Submenu<R>> {
    let view = command_menu(app, "View", commands)?;
    #[cfg(target_os = "macos")]
    let view = view.separator().fullscreen();
    view.build()
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let close_pane = MenuItemBuilder::with_id("close-pane", "Close Pane").accelerator(CLOSE_PANE).build(app)?;
    let check_updates = MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…").accelerator("CmdOrCtrl+,").build(app)?;
    let mut file = SubmenuBuilder::new(app, "File").item(&close_pane);
    #[cfg(target_os = "macos")]
    {
        let close_window =
            MenuItemBuilder::with_id("close-window", "Close Window").accelerator("Cmd+Shift+W").build(app)?;
        file = file.item(&close_window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        file = file.separator().item(&settings).item(&check_updates).separator().quit();
    }
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().build()?;
    let commands = commands();
    let tab = command_menu(app, "Tab", &commands)?.build()?;
    let pane = command_menu(app, "Pane", &commands)?.build()?;
    let view = view_menu(app, &commands)?;

    let mut menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(app, "Wings")
            .about(None)
            .item(&check_updates)
            .separator()
            .item(&settings)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .separator()
            .quit()
            .build()?;
        menu = menu.item(&app_menu).item(&file.build()?).item(&edit).item(&tab).item(&pane).item(&view).item(&window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        menu = menu.item(&file.build()?).item(&edit).item(&tab).item(&pane).item(&view).item(&window);
    }
    menu.build()
}

pub fn handle<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        id @ ("close-pane" | "check-updates") => {
            let _ = app.emit("menu", id);
        }
        id if id.starts_with("cmd:") => {
            let _ = app.emit("menu", id);
        }
        "close-window" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.close();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Shortcut;

    fn find(id: &str) -> Command {
        commands().into_iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn accelerators_follow_the_shortcuts_the_window_shows() {
        assert_eq!(accelerator(&find("new-tab"), true).as_deref(), Some("Cmd+t"));
        assert_eq!(accelerator(&find("split-down"), true).as_deref(), Some("Cmd+Shift+d"));
        assert_eq!(accelerator(&find("split-down"), false).as_deref(), Some("Ctrl+Shift+Alt+d"));
        assert_eq!(accelerator(&find("focus-left"), true).as_deref(), Some("Cmd+Alt+ArrowLeft"));
        assert_eq!(accelerator(&find("font-up"), true).as_deref(), Some("Cmd+Equal"));
        assert_eq!(accelerator(&find("settings"), false).as_deref(), Some("CmdOrCtrl+,"));
        assert_eq!(accelerator(&find("history"), true), None);
    }

    /// A bad accelerator fails when the menu is built, which is when Wings starts. The grammar is the one the
    /// menu uses (`muda`), which the global shortcut parser shares.
    #[test]
    fn every_accelerator_parses() {
        for c in commands() {
            for mac in [true, false] {
                if let Some(a) = accelerator(&c, mac) {
                    a.replace("CmdOrCtrl", "Ctrl").parse::<Shortcut>().unwrap_or_else(|e| panic!("{} ({a}): {e}", c.id));
                }
            }
        }
    }

    #[test]
    fn only_the_three_menus_are_named_and_each_has_items() {
        let all = commands();
        for name in ["Tab", "Pane", "View"] {
            assert!(all.iter().any(|c| c.menu.as_deref() == Some(name)), "{name} has no items");
        }
        assert!(all.iter().all(|c| matches!(c.menu.as_deref(), None | Some("Tab" | "Pane" | "View"))));
    }

    #[test]
    fn menu_commands_are_unique_and_the_default_hotkey_parses() {
        let all = commands();
        for c in &all {
            assert_eq!(all.iter().filter(|o| o.id == c.id).count(), 1, "{} twice", c.id);
        }
        crate::hotkey::default_accelerator().parse::<Shortcut>().unwrap();
    }
}
