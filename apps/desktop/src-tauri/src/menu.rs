//! The app menu. Same as Tauri's default except for closing: in a terminal app the close
//! shortcut should close the focused pane, not the whole window with every terminal in it.

use tauri::{
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
    AppHandle, Emitter, Manager, Runtime,
};

// Plain Ctrl+W deletes a word in the shell, so other platforms use Ctrl+Shift+W like Windows Terminal.
#[cfg(target_os = "macos")]
const CLOSE_PANE: &str = "Cmd+W";
#[cfg(not(target_os = "macos"))]
const CLOSE_PANE: &str = "Ctrl+Shift+W";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let close_pane = MenuItemBuilder::with_id("close-pane", "Close Pane").accelerator(CLOSE_PANE).build(app)?;
    let check_updates = MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
    let mut file = SubmenuBuilder::new(app, "File").item(&close_pane);
    #[cfg(target_os = "macos")]
    {
        let close_window =
            MenuItemBuilder::with_id("close-window", "Close Window").accelerator("Cmd+Shift+W").build(app)?;
        file = file.item(&close_window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        file = file.separator().item(&check_updates).separator().quit();
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

    let mut menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(app, "Wings")
            .about(None)
            .item(&check_updates)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .separator()
            .quit()
            .build()?;
        let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;
        menu = menu.item(&app_menu).item(&file.build()?).item(&edit).item(&view).item(&window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        menu = menu.item(&file.build()?).item(&edit).item(&window);
    }
    menu.build()
}

pub fn handle<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        id @ ("close-pane" | "check-updates") => {
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
