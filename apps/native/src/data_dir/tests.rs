use std::{collections::BTreeMap, fs, path::Path};

use serde_json::json;

use super::{REPLACEMENT_STATE, desktop, migrate, state_file_in};
use crate::saved::{self, SavedDir, SavedLayout, SavedPluginTab, SavedProject, SavedTab, Settings};

/// A desktop `workspace.json` with every key the desktop writes (`SavedWorkspace` in `App.tsx`, and main's
/// `claudeAccounts`), in the shapes the real file has, with made-up values.
fn desktop_workspace() -> serde_json::Value {
    let mut workspace = json!({
        "version": 1,
        "activeSpaceId": "/work/both",
        "activeTabBySpace": { "/work/both": "t2", "/work/desk": "v1" },
        "spaceOrder": ["/work/both", "/work/desk"],
        "sidebarOpen": false,
        "widths": { "left": 319, "right": 410, "diff": 1007 },
        "pinnedButtons": ["cyclops:tracker"],
        "fontSize": 15,
        "terminalKeys": { "optionAsMeta": "both", "shiftReturnSendsMetaReturn": false },
        "globalHotkey": { "enabled": true, "accelerator": "Ctrl+Alt+KeyK" },
        "quickTerminalKey": { "enabled": false, "accelerator": "Ctrl+Backquote" },
        "terminalLook": {
            "fontFamily": "",
            "lineHeight": 1.2,
            "cursorStyle": "bar",
            "cursorBlink": true,
            "cursorInactiveStyle": "outline",
            "scrollback": 5000,
            "copyOnSelect": true,
            "minimumContrastRatio": 1,
            "images": false,
            "clipboardWrite": true
        },
        "tabs": [
            { "id": "t1", "spaceId": "/work/both", "layout": { "kind": "pane", "id": "p1" }, "focusedPane": "p1", "zoomedPane": null },
            {
                "id": "t2",
                "spaceId": "/work/both",
                "layout": {
                    "kind": "split", "id": "s1", "dir": "row", "ratio": 0.4,
                    "a": { "kind": "pane", "id": "p2", "name": "server" },
                    "b": { "kind": "pane", "id": "p3" }
                },
                "focusedPane": "p3",
                "zoomedPane": "p3",
                "name": "Dev"
            },
            { "id": "t3", "spaceId": "/work/desk", "layout": { "kind": "pane", "id": "p4" }, "focusedPane": "p4", "zoomedPane": null }
        ],
        "viewTabs": [
            { "id": "v2", "spaceId": "/work/desk", "view": { "pluginId": "tasks", "viewId": "board" }, "at": 5 },
            { "id": "v1", "spaceId": "/work/desk", "view": { "pluginId": "tasks", "viewId": "issue" }, "args": { "id": 7 }, "at": 2 }
        ],
        "projectTabs": [{ "id": "pt", "spaceId": "/work/both", "project": true, "at": 3 }],
        "sessions": { "p1": "session-1", "p2": null },
        "sessionKinds": { "p1": "claude" },
        "asleep": [],
        "sleep": { "enabled": true, "minutes": 15 },
        "notifySound": { "enabled": false, "sound": "pop", "watching": true },
        "agentsRunCommands": true,
        "agentsMessage": false,
        "agentTerminalsOpen": "float",
        "newTabPage": "terminal",
        "newProjectOpens": "project",
        "collapsedSections": [],
        "projectSort": "worked",
        "olderOpen": true,
        "lastActive": { "/work/both": 2000, "/work/desk": 1000 },
        "notifications": [{ "at": 1, "detail": "", "id": "n1", "kind": "done", "paneId": "p1", "plugin": null, "project": "/work/both", "read": true, "title": "Done" }],
        "sessionArgs": { "p1": ["--model", "opus"] },
        "cwds": { "p1": "/work/both", "p2": "/work/both/api", "p3": "/work/both/web", "p4": "/work/desk" },
        "paneIds": { "p1": "live-1" },
        "histories": { "p1": "aaaa-1", "p2": "aaaa-2", "p3": "../escape", "p4": "aaaa-4" },
        "agentRuns": { "p1": { "kind": "claude", "pid": 1, "startedAt": 1 } },
        "openedBy": { "p1": "tasks" },
        "claudeAccounts": { "p1": { "name": "work" } }
    });
    // Set apart: one more level in the literal is past `json!`'s recursion limit.
    workspace["claudeAccounts"] = json!({ "p1": { "name": "work" }, "p2": { "name": 5 }, "p3": { "name": "../x" }, "p4": { "name": null } });
    workspace
}

/// `pane` run under the Claude Code account `name`, `None` being the default one.
fn pane_as(cwd: &str, history: &str, name: Option<&str>) -> SavedLayout {
    SavedLayout::Pane {
        cwd: Some(cwd.into()),
        history: Some(history.into()),
        name: None,
        claude_account: Some(wings_spaces::claude_accounts::AccountPick {
            name: name.map(Into::into),
        }),
    }
}

fn pane(cwd: &str, history: Option<&str>, name: Option<&str>) -> SavedLayout {
    SavedLayout::Pane {
        cwd: Some(cwd.into()),
        history: history.map(Into::into),
        name: name.map(Into::into),
        claude_account: None,
    }
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

/// Every file under `dir` with its bytes, to show a folder was left exactly as it was.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.insert(path.display().to_string(), fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[test]
fn the_side_by_side_build_keeps_workspace_json_in_its_own_folder() {
    const { assert!(!super::REPLACEMENT, "tests run the side-by-side build") };
    assert_eq!(super::APP_NAME, "Wings Native");
    let dir = Path::new("/data");
    assert_eq!(state_file_in(dir, false), dir.join("workspace.json"));
    assert_eq!(state_file_in(dir, true), dir.join("native-workspace.json"));
    let state = super::state_file().unwrap();
    assert!(
        state.ends_with("Wings Native/workspace.json")
            || state.ends_with("wings-native/workspace.json")
    );
    assert_eq!(
        super::desktop_dir().unwrap().file_name().unwrap(),
        "dev.wings.app"
    );
}

#[test]
fn the_desktops_workspace_becomes_native_tabs_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("workspace.json");
    write_json(&file, &desktop_workspace());
    let workspace = desktop::read(&file).expect("the desktop's file parses");

    let projects = workspace.projects();
    assert_eq!(
        projects["/work/both"],
        SavedProject {
            tabs: vec![
                SavedTab {
                    // The desktop's `claudeAccounts`: the pane opened as the work account stays it.
                    layout: pane_as("/work/both", "aaaa-1", Some("work")),
                    focused: 0,
                    zoomed: None,
                },
                SavedTab {
                    layout: SavedLayout::Split {
                        dir: SavedDir::Row,
                        ratio: 0.4,
                        // An account entry that isn't `{ name }` with an account name, or null, is dropped.
                        a: Box::new(pane("/work/both/api", Some("aaaa-2"), Some("server"))),
                        // A history id that isn't a plain name is dropped, as both apps drop it.
                        b: Box::new(pane("/work/both/web", None, None)),
                    },
                    focused: 1,
                    zoomed: Some(1),
                },
            ],
            active: Some(1),
            ..SavedProject::default()
        }
    );
    assert_eq!(
        projects["/work/desk"],
        SavedProject {
            tabs: vec![SavedTab {
                layout: pane_as("/work/desk", "aaaa-4", None),
                focused: 0,
                zoomed: None,
            }],
            active: None,
            plugin_tabs: vec![
                SavedPluginTab {
                    plugin: "tasks".into(),
                    view: "issue".into(),
                    args: Some(json!({ "id": 7 })),
                    state: None,
                },
                SavedPluginTab {
                    plugin: "tasks".into(),
                    view: "board".into(),
                    args: None,
                    state: None,
                },
            ],
            active_plugin: Some(0),
            issues_closed: false,
        }
    );

    let settings = workspace.settings();
    assert_eq!(settings.font_size, Some(15.0));
    assert_eq!(settings.sidebar_open, Some(false));
    assert_eq!(settings.sidebar_width, Some(319.0));
    assert_eq!(settings.right_width, Some(410.0));
    assert_eq!(
        settings.font_family, None,
        "an empty family is the default font"
    );
    assert_eq!(settings.cursor_style.as_deref(), Some("bar"));
    assert_eq!(settings.scrollback, Some(5000));
    assert_eq!(settings.images, Some(false));
    assert_eq!(settings.option_as_meta.as_deref(), Some("both"));
    assert_eq!(settings.shift_return_meta, Some(false));
    assert_eq!(settings.quick_terminal, Some(false));
    assert_eq!(settings.global_hotkey.unwrap().accelerator, "Ctrl+Alt+KeyK");
    assert_eq!(settings.new_tab.as_deref(), Some("terminal"));
    assert_eq!(settings.new_project_opens.as_deref(), Some("project"));
    assert_eq!(settings.project_sort.as_deref(), Some("worked"));
    assert_eq!(settings.notify_sound.unwrap().sound.as_deref(), Some("pop"));
    assert_eq!(settings.agents_message, Some(false));
    assert_eq!(
        settings.pinned_buttons,
        Some(vec!["cyclops:tracker".to_string()])
    );
    assert_eq!(settings.last_active["/work/both"], 2000);

    let mut ids = desktop::history_ids(&file);
    ids.sort();
    assert_eq!(ids, ["../escape", "aaaa-1", "aaaa-2", "aaaa-4"]);
}

#[test]
fn a_desktop_file_the_desktop_would_not_load_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("workspace.json");
    write_json(&file, &json!({ "version": 2, "tabs": [] }));
    assert!(desktop::read(&file).is_none());
    fs::write(&file, b"{ half a file").unwrap();
    assert!(desktop::read(&file).is_none());
    assert!(desktop::read(&dir.path().join("missing.json")).is_none());
}

/// A side-by-side folder and a desktop folder, as each app leaves them.
fn fixture(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let (from, to) = (root.join("Wings Native"), root.join("dev.wings.app"));
    let mut native = BTreeMap::new();
    native.insert(
        "/work/both".to_owned(),
        SavedProject {
            tabs: vec![SavedTab {
                layout: pane("/work/both/native", Some("18e5-0"), None),
                focused: 0,
                zoomed: None,
            }],
            active: Some(0),
            ..SavedProject::default()
        },
    );
    native.insert(
        "/work/native-only".to_owned(),
        SavedProject {
            tabs: vec![SavedTab {
                layout: pane("/work/native-only", Some("18e5-1"), None),
                focused: 0,
                zoomed: None,
            }],
            active: Some(0),
            ..SavedProject::default()
        },
    );
    let settings = Settings {
        font_size: Some(13.0),
        font_family: Some("Native Mono".into()),
        seen_plugins: vec!["hud".into()],
        diff_wrap: Some(true),
        last_active: BTreeMap::from([("/work/both".into(), 3000), ("/work/desk".into(), 500)]),
        ..Settings::default()
    };
    saved::write_all(&from.join("workspace.json"), settings, native).unwrap();
    write_json(
        &from.join("spaces.json"),
        &json!([
            { "id": "/work/both", "name": "both", "path": "/work/both" },
            { "id": "/work/native-only", "name": "native-only", "path": "/work/native-only", "pinned": true }
        ]),
    );
    fs::create_dir_all(from.join("scrollback")).unwrap();
    fs::write(from.join("scrollback/18e5-0"), b"native output 0").unwrap();
    fs::write(from.join("scrollback/18e5-1"), b"native output 1").unwrap();
    fs::write(from.join("scrollback/aaaa-1"), b"a different aaaa-1").unwrap();
    fs::write(from.join("scrollback/not..plain"), b"skipped").unwrap();
    fs::create_dir_all(from.join("terminal-server/bin")).unwrap();
    fs::write(from.join("terminal-server/v3.log"), b"").unwrap();

    write_json(&to.join("workspace.json"), &desktop_workspace());
    write_json(
        &to.join("spaces.json"),
        &json!([
            { "id": "/work/both", "name": "both", "path": "/work/both", "claudeAccount": "work", "addedAt": 1 },
            { "id": "/work/desk", "name": "desk", "path": "/work/desk" }
        ]),
    );
    fs::create_dir_all(to.join("scrollback")).unwrap();
    fs::write(to.join("scrollback/aaaa-1"), b"desktop output 1").unwrap();
    write_json(&to.join("plugins.json"), &json!({ "plugins": {} }));
    (from, to)
}

#[test]
fn the_first_replacement_start_brings_both_sides_over_and_backs_up_what_it_replaces() {
    let root = tempfile::tempdir().unwrap();
    let (from, to) = fixture(root.path());
    let native_before = snapshot(&from);
    let desktop_workspace_before = fs::read(to.join("workspace.json")).unwrap();
    let spaces_before = fs::read(to.join("spaces.json")).unwrap();
    let plugins_before = fs::read(to.join("plugins.json")).unwrap();

    let report = migrate::migrate(&from, &to, 1234).unwrap();
    let backup = to.join("backups/native-switch-1234");
    assert_eq!(
        report,
        migrate::Report {
            already: false,
            desktop_projects: 2,
            native_projects: 1,
            added_spaces: 1,
            copied_scrollback: 2,
            scrollback_conflicts: 1,
            backup: Some(backup.clone()),
        }
    );

    // The desktop's tabs win in a project both had; the side-by-side app's other projects come along.
    let state = to.join(REPLACEMENT_STATE);
    let (settings, projects) = saved::read_all(&state).unwrap();
    assert_eq!(projects.len(), 3);
    assert_eq!(projects["/work/both"].tabs.len(), 2);
    assert_eq!(
        projects["/work/native-only"].tabs[0].layout,
        pane("/work/native-only", Some("18e5-1"), None)
    );
    assert_eq!(
        saved::load(&state, "/work/desk").unwrap().active_plugin,
        Some(0)
    );
    // The desktop's settings win; what only the native app keeps stays; the later of each project's times.
    assert_eq!(settings.font_size, Some(15.0));
    assert_eq!(settings.font_family, Some("Native Mono".into()));
    assert_eq!(settings.seen_plugins, ["hud"]);
    assert_eq!(settings.diff_wrap, Some(true));
    assert_eq!(settings.cursor_style.as_deref(), Some("bar"));
    assert_eq!(settings.last_active["/work/both"], 3000);
    assert_eq!(settings.last_active["/work/desk"], 1000);

    // spaces.json gets the side-by-side-only project, each entry as written, and the desktop's file is backed up.
    let spaces: serde_json::Value =
        serde_json::from_slice(&fs::read(to.join("spaces.json")).unwrap()).unwrap();
    assert_eq!(
        spaces,
        json!([
            { "id": "/work/both", "name": "both", "path": "/work/both", "claudeAccount": "work", "addedAt": 1 },
            { "id": "/work/desk", "name": "desk", "path": "/work/desk" },
            { "id": "/work/native-only", "name": "native-only", "path": "/work/native-only", "pinned": true }
        ])
    );
    let parsed: Vec<wings_spaces::spaces::Space> =
        serde_json::from_slice(&fs::read(to.join("spaces.json")).unwrap()).unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(fs::read(backup.join("spaces.json")).unwrap(), spaces_before);

    // Saved output comes over under new names; a name the desktop already used keeps the desktop's.
    assert_eq!(
        fs::read(to.join("scrollback/18e5-0")).unwrap(),
        b"native output 0"
    );
    assert_eq!(
        fs::read(to.join("scrollback/18e5-1")).unwrap(),
        b"native output 1"
    );
    assert_eq!(
        fs::read(to.join("scrollback/aaaa-1")).unwrap(),
        b"desktop output 1"
    );
    assert!(!to.join("scrollback/not..plain").exists());
    // The terminal server's sockets and binary copy belong to a running process, so they stay.
    assert!(!to.join("terminal-server").exists());

    // The desktop can still load its own files, unchanged, and the side-by-side folder is untouched.
    assert_eq!(
        fs::read(to.join("workspace.json")).unwrap(),
        desktop_workspace_before
    );
    assert!(desktop::read(&to.join("workspace.json")).is_some());
    assert_eq!(fs::read(to.join("plugins.json")).unwrap(), plugins_before);
    assert_eq!(snapshot(&from), native_before);
    assert!(to.join(migrate::MARKER).exists());
}

#[test]
fn a_later_start_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (from, to) = fixture(root.path());
    migrate::migrate(&from, &to, 1).unwrap();
    // A project removed after the switch stays removed.
    write_json(
        &to.join("spaces.json"),
        &json!([{ "id": "/work/desk", "name": "desk", "path": "/work/desk" }]),
    );
    let after = snapshot(&to);

    let again = migrate::migrate(&from, &to, 2).unwrap();
    assert!(again.already);
    assert_eq!(snapshot(&to), after);
    assert!(!to.join("backups/native-switch-2").exists());
}

#[test]
fn a_run_cut_short_after_the_workspace_finishes_without_redoing_it() {
    let root = tempfile::tempdir().unwrap();
    let (from, to) = fixture(root.path());
    migrate::migrate(&from, &to, 1).unwrap();
    // As if the app stopped before the marker: the state it already saved since is kept.
    fs::remove_file(to.join(migrate::MARKER)).unwrap();
    saved::save(
        &to.join(REPLACEMENT_STATE),
        "/work/new",
        SavedProject::default(),
        Settings::default(),
    )
    .unwrap();
    let state = fs::read(to.join(REPLACEMENT_STATE)).unwrap();

    let report = migrate::migrate(&from, &to, 2).unwrap();
    assert_eq!((report.added_spaces, report.copied_scrollback), (0, 0));
    assert_eq!(fs::read(to.join(REPLACEMENT_STATE)).unwrap(), state);
    let spaces: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(to.join("spaces.json")).unwrap()).unwrap();
    assert_eq!(spaces.len(), 3, "no project is added twice");
}

#[test]
fn a_desktop_user_who_never_ran_the_side_by_side_app_gets_their_desktop_tabs() {
    let root = tempfile::tempdir().unwrap();
    let to = root.path().join("dev.wings.app");
    write_json(&to.join("workspace.json"), &desktop_workspace());
    let report = migrate::migrate(&root.path().join("Wings Native"), &to, 1).unwrap();
    assert_eq!((report.desktop_projects, report.native_projects), (2, 0));
    assert_eq!(report.backup, None);
    assert_eq!(
        saved::load(&to.join(REPLACEMENT_STATE), "/work/both")
            .unwrap()
            .tabs
            .len(),
        2
    );
    assert!(!root.path().join("Wings Native").exists());
}

#[test]
fn pruning_on_the_desktops_folder_keeps_the_output_the_desktops_tabs_refer_to() {
    let root = tempfile::tempdir().unwrap();
    let (from, to) = fixture(root.path());
    migrate::migrate(&from, &to, 1).unwrap();
    fs::write(to.join("scrollback/aaaa-2"), b"desktop output 2").unwrap();
    fs::write(to.join("scrollback/stale"), b"nobody's").unwrap();
    // The native app closed every tab of /work/both, whose desktop panes saved aaaa-1 and aaaa-2.
    saved::save(
        &to.join(REPLACEMENT_STATE),
        "/work/both",
        SavedProject::default(),
        Settings::default(),
    )
    .unwrap();

    saved::prune_histories(&to.join(REPLACEMENT_STATE));
    assert!(to.join("scrollback/aaaa-1").exists());
    assert!(to.join("scrollback/aaaa-2").exists());
    assert!(
        to.join("scrollback/18e5-1").exists(),
        "still a native pane's"
    );
    assert!(!to.join("scrollback/stale").exists());
}
