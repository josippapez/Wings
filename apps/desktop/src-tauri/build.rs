// Listing the app's commands turns on Tauri's permission check for them, so only windows whose
// capability allows them (the main window, see capabilities/default.json) can call them. Plugin
// panels show outside web pages and get none.
const COMMANDS: &[&str] = &[
    "spaces_list",
    "spaces_add",
    "spaces_remove",
    "sessions_list",
    "pane_create",
    "pane_write",
    "pane_resize",
    "pane_close",
    "panes_reset",
    "pane_focus",
    "agents_list",
    "pane_info",
    "git_status",
    "git_refresh",
    "workspace_load",
    "workspace_save",
    "plugins_list",
    "plugin_install_file",
    "plugin_install_github",
    "plugin_update",
    "plugin_latest_version",
    "plugin_set_enabled",
    "plugin_remove",
    "plugin_panel_toggle",
    "plugin_secret_set",
    "plugin_secret_delete",
    "plugin_secret_has",
    "plugin_fetch",
    "plugin_exec",
    "plugin_transcript",
    "plugin_open_url",
    "bench_mode",
    "bench_report",
];

fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("tauri build");
}
