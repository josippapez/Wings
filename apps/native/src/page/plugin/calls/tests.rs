//! Each `wings.*` call of a native frame: what it does when the manifest grants it, what it says when it doesn't, and
//! what it says of bad arguments. The words are the desktop's.

use std::{fs, sync::mpsc, time::Duration};

use serde_json::{Value, json};
use wings_spaces::claude_accounts::AccountPick;

use crate::page::{
    plugin::{
        PanePort, PluginHost,
        testing::{
            Caller, Frame, Rig, Seen, WAIT, call_at_start, call_from_main, exec, hears, idle, open_pane, pane, show,
        },
        tests::Root,
    },
    runtime::{
        Store,
        fake::{self, Cx, Guest},
    },
};

fn tmp() -> std::path::PathBuf {
    std::env::temp_dir().canonicalize().unwrap()
}

fn panes() -> Vec<crate::page::plugin::PaneEntry> {
    vec![pane("1", "/a"), pane("2", "/b")]
}

#[test]
fn a_host_without_a_manifest_refuses_every_call() {
    let root = Root::new();
    let host = PluginHost::new(root.config("stray"));
    let rig = Rig {
        host,
        env: crate::page::plugin::testing::FakeEnv::new(),
        root,
    };
    for (method, params) in [
        ("exec", exec("echo", &[])),
        ("setBadge", json!({ "paneId": "1", "badge": { "label": "x" } })),
        ("openUrl", json!({ "url": "https://github.com/" })),
        ("ports", json!({})),
        ("statusline", json!({})),
        ("notify", json!({ "title": "x", "body": "" })),
        (
            "transcript",
            json!({ "sessionId": "3c21b0c5-8113-4f55-9493-fa156c3fa369", "types": ["user"] }),
        ),
    ] {
        assert_eq!(
            rig.refuses(method, params),
            "stray is turned off or not installed",
            "{method}"
        );
    }
}

// onPanes

#[test]
fn every_frame_hears_the_panes_as_the_desktop_lists_them_when_it_starts_and_when_they_change() {
    let rig = Rig::new(json!({}), panes());
    let frame = Frame::start(&rig.host, &hears("panes"));
    // The list that was there before the frame started comes once it has started.
    let report = frame.report("the first list", |r| {
        r["heard"].as_array().is_some_and(|h| h.len() == 1)
    });
    assert_eq!(
        report["heard"][0],
        json!([
            { "paneId": "1", "cwd": "/a", "command": "zsh", "project": "proj", "projectId": "/proj", "session": null, "focused": false },
            { "paneId": "2", "cwd": "/b", "command": "zsh", "project": "proj", "projectId": "/proj", "session": null, "focused": false },
        ])
    );

    // The same list again is nothing new; a changed one is heard, with an agent's session as `WingsPane` has it.
    rig.host.publish_panes(panes());
    let mut changed = panes();
    changed[1].cwd = "/elsewhere".into();
    changed[1].focused = true;
    changed[0].session = Some(crate::page::plugin::SessionEntry {
        session_id: "3c21b0c5-8113-4f55-9493-fa156c3fa369".into(),
        kind: "claude".into(),
        name: Some("fix it".into()),
        state: "working".into(),
        started_at: 1_700_000_000_000,
    });
    rig.host.publish_panes(changed);
    let report = frame.report("the changed list", |r| {
        r["heard"].as_array().is_some_and(|h| h.len() == 2)
    });
    assert_eq!(report["heard"][1][1]["cwd"], "/elsewhere");
    assert_eq!(report["heard"][1][1]["focused"], true);
    assert_eq!(
        report["heard"][1][0]["session"],
        json!({ "sessionId": "3c21b0c5-8113-4f55-9493-fa156c3fa369", "kind": "claude", "name": "fix it", "state": "working", "startedAt": 1_700_000_000_000u64 })
    );
}


// setBadge

fn badges() -> Rig {
    Rig::new(json!({ "contributes": { "ui": ["badges"] } }), panes())
}

#[test]
fn a_badge_is_cleaned_and_handed_to_the_app_and_a_repeat_is_not() {
    let rig = badges();
    let badge = json!({ "label": "localhost:5173", "tone": "success", "actions": [{ "id": "open", "label": "Open", "primary": true }], "extra": 1 });
    rig.allows("setBadge", json!({ "paneId": "1", "badge": badge }));
    rig.allows("setBadge", json!({ "paneId": "1", "badge": null }));
    let seen = rig.env.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let Seen::Badge(plugin, pane, Some(badge)) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!((plugin.as_str(), pane.as_str()), ("probe", "1"));
    assert_eq!(badge.label, "localhost:5173");
    assert_eq!(badge.actions[0].id, "open");
    assert_eq!(seen[1], Seen::Badge("probe".into(), "1".into(), None));
}

#[test]
fn a_badge_with_no_label_clears_the_pane_and_a_failed_call_is_sent_again() {
    let rig = badges();
    rig.allows("setBadge", json!({ "paneId": "2", "badge": { "tone": "info" } }));
    assert_eq!(
        rig.env.seen(),
        [Seen::Badge("probe".into(), "2".into(), None)]
    );
    // A pane that isn't there is refused.
    assert_eq!(
        rig.refuses("setBadge", json!({ "paneId": "9", "badge": { "label": "x" } })),
        "no pane 9"
    );
}

#[test]
fn a_badge_needs_badges_declared_and_a_pane_that_is_there() {
    let rig = Rig::new(json!({ "contributes": { "ui": ["status"] } }), panes());
    assert_eq!(
        rig.refuses("setBadge", json!({ "paneId": "1", "badge": { "label": "x" } })),
        "probe must declare \"badges\" in contributes.ui"
    );
    assert_eq!(
        badges().refuses("setBadge", json!({ "paneId": "7", "badge": { "label": "x" } })),
        "no pane 7"
    );
    assert!(rig.env.seen().is_empty());
}

// onAction

/// A main that keeps the last action it was told of, fails `boom`, and settles `slow` after a wait.
struct Clicks(Vec<(u32, u32)>);

impl Guest for Clicks {
    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, token: u32, text: &str) {
        if kind != "action" {
            return;
        }
        let click: Value = serde_json::from_str(text).unwrap();
        let link = cx.link().clone();
        let last = json!({ "paneId": click["paneId"], "actionId": click["actionId"] });
        link.storage(Store::Set("last".into(), last.to_string())).unwrap();
        match click["actionId"].as_str() {
            Some("boom") => {
                cx.call("actionDone", json!({ "token": token, "error": "It broke" }));
            }
            Some("slow") => {
                link.storage(Store::Set("second".into(), json!("done").to_string())).unwrap();
                let timer = cx.later(100);
                self.0.push((timer, token));
            }
            _ => {
                cx.call("actionDone", json!({ "token": token, "error": null }));
            }
        }
    }

    fn timer(&mut self, cx: &mut Cx<'_>, id: u32) {
        if let Some(&(_, token)) = self.0.iter().find(|(timer, _)| *timer == id) {
            cx.call("actionDone", json!({ "token": token, "error": null }));
        }
    }
}

fn action(host: &PluginHost, pane: &str, id: &str) -> Result<(), String> {
    let (done, finished) = mpsc::channel();
    host.send_action(pane, id, move |result| {
        let _ = done.send(result);
    });
    finished.recv_timeout(WAIT).expect("the action to settle")
}

#[test]
fn a_click_on_a_badge_button_reaches_the_main_frames_listeners_and_settles_when_they_all_have() {
    let rig = badges();
    rig.host
        .start_main(&fake::source("main", || Box::new(Clicks(Vec::new()))), |_| {})
        .unwrap();
    assert_eq!(action(&rig.host, "1", "open"), Ok(()));
    assert_eq!(
        rig.root.saved("probe", "last"),
        Some(json!({ "paneId": "1", "actionId": "open" }))
    );
    // The spinner stays until the listener is done.
    assert_eq!(action(&rig.host, "2", "slow"), Ok(()));
    assert_eq!(rig.root.saved("probe", "second"), Some(json!("done")));
    // A listener that throws shows its message in the card.
    assert_eq!(action(&rig.host, "1", "boom"), Err("It broke".into()));
}

#[test]
fn a_click_fails_at_once_without_a_main_and_when_the_plugin_is_turned_off() {
    let rig = badges();
    assert_eq!(
        action(&rig.host, "1", "open"),
        Err("plugin probe is not running".into())
    );

    rig.host.start_main(&idle("main"), |_| {}).unwrap();
    let (done, finished) = mpsc::channel();
    rig.host.send_action("1", "open", move |result| {
        let _ = done.send(result);
    });
    assert!(finished.recv_timeout(Duration::from_millis(200)).is_err());
    drop(rig.host);
    assert_eq!(
        finished.recv_timeout(WAIT).unwrap(),
        Err("The plugin was turned off".into())
    );
}

// setPaneStatus

fn statuses() -> Rig {
    Rig::new(
        json!({ "contributes": {
            "ui": ["status"],
            "sidebars": [{ "id": "hud", "title": "HUD", "page": "hud.html" }],
            "views": [{ "id": "board", "title": "Board", "page": "board.html", "opens": ["tab"] }],
        } }),
        panes(),
    )
}

#[test]
fn a_status_is_checked_and_handed_to_the_app_and_null_takes_it_away() {
    let rig = statuses();
    let set = |status: Value| rig.allows("setPaneStatus", json!({ "paneId": "1", "status": status }));
    set(json!({ "items": [{ "label": "5h", "value": "23%", "percent": 23, "tone": "warning" }], "rows": [{ "label": "Resets", "value": "in 2h" }], "open": "hud" }));
    set(json!({ "items": [{ "value": "x" }], "open": "board" }));
    set(json!(null));
    let seen = rig.env.seen();
    assert_eq!(seen.len(), 3, "{seen:?}");
    let Seen::Status(plugin, pane, Some(status)) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!((plugin.as_str(), pane.as_str()), ("probe", "1"));
    assert_eq!(status.items[0].value, "23%");
    assert_eq!(status.open.as_deref(), Some("hud"));
    assert_eq!(seen[2], Seen::Status("probe".into(), "1".into(), None));
}

#[test]
fn a_status_needs_status_declared_a_pane_and_to_keep_within_the_limits() {
    let rig = statuses();
    let refused = |pane: &str, status: Value| {
        rig.refuses("setPaneStatus", json!({ "paneId": pane, "status": status }))
    };
    assert_eq!(
        refused("8", json!({ "items": [{ "value": "x" }] })),
        "no pane 8"
    );
    assert_eq!(
        refused("1", json!({ "items": [] })),
        "setPaneStatus: items must be a non-empty array"
    );
    assert_eq!(
        refused("1", json!({ "items": [{ "value": "x".repeat(33) }] })),
        "setPaneStatus: items[0].value is over 32 characters"
    );
    assert_eq!(
        refused("1", json!({ "items": [{ "value": "x", "percent": 120 }] })),
        "setPaneStatus: items[0].percent must be a number from 0 to 100"
    );
    assert_eq!(
        refused("1", json!({ "items": [{ "value": "x" }], "open": "nope" })),
        "setPaneStatus: open must be the id of one of the plugin's sidebars or views (hud, board)"
    );
    // Nothing at all is not a status, though `null` is the way to remove one.
    rig.allows("setPaneStatus", json!({ "paneId": "1", "status": { "items": [{ "value": "x" }] } }));
    assert_eq!(
        rig.refuses("setPaneStatus", json!({ "paneId": "1" })),
        "setPaneStatus: status must be an object or null"
    );
    assert_eq!(rig.env.seen().len(), 1);

    let undeclared = Rig::new(json!({ "contributes": { "ui": ["badges"] } }), panes());
    assert_eq!(
        undeclared.refuses("setPaneStatus", json!({ "paneId": "1", "status": { "items": [{ "value": "x" }] } })),
        "probe must declare \"status\" in contributes.ui"
    );
}

// exec

fn exec_rig(entries: &[&str]) -> Rig {
    let rig = Rig::new(json!({ "permissions": { "exec": entries } }), panes());
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    rig
}

#[test]
fn exec_runs_a_program_the_manifest_names_and_resolves_its_output() {
    let rig = exec_rig(&["echo"]);
    assert_eq!(
        rig.allows("exec", exec("echo", &["hi", "3"])),
        json!({ "code": 0, "stdout": "hi 3\n", "stderr": "" })
    );
    let mut with_options = exec("echo", &["in"]);
    with_options["cwd"] = json!(tmp());
    with_options["timeoutMs"] = json!(5000);
    assert_eq!(rig.allows("exec", with_options)["code"], 0);
}

#[test]
fn exec_streams_each_line_while_it_runs_when_asked() {
    let rig = exec_rig(&["echo"]);
    let mut params = exec("echo", &["one"]);
    params["stream"] = json!(true);
    let (result, lines) = Caller::start(&rig.host).streamed("exec", params);
    assert_eq!(lines, ["one"]);
    assert_eq!(result.unwrap()["stdout"], "one\n");
}

/// A frame that runs `sleep` and counts timers of 20 ms meanwhile, and shows how long three of them took.
struct Ticks {
    started: std::time::Instant,
    ticks: u32,
    ticked: Option<u128>,
    code: Option<Value>,
}

impl Ticks {
    fn done(&self, cx: &mut Cx<'_>) {
        if let (Some(ticked), Some(code)) = (self.ticked, &self.code) {
            show(cx, json!({ "ticked": ticked, "code": code }));
        }
    }
}

impl Guest for Ticks {
    fn start(&mut self, cx: &mut Cx<'_>) {
        cx.call("exec", exec("sleep", &["0.6"]));
        cx.later(20);
    }

    fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
        self.ticks += 1;
        if self.ticks < 3 {
            cx.later(20);
        } else {
            self.ticked = Some(self.started.elapsed().as_millis());
            self.done(cx);
        }
    }

    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
        if kind == "resolve" {
            self.code = Some(serde_json::from_str::<Value>(text).unwrap()["code"].clone());
            self.done(cx);
        }
    }
}

#[test]
fn exec_does_not_hold_the_frame_up_while_the_program_runs() {
    let rig = exec_rig(&["sleep"]);
    let source = fake::source("ticks", || {
        Box::new(Ticks { started: std::time::Instant::now(), ticks: 0, ticked: None, code: None })
    });
    let result = Frame::start(&rig.host, &source).report("the program and the timers", |_| true);
    assert_eq!(result["code"], 0);
    let ticked = result["ticked"].as_u64().unwrap();
    assert!(
        ticked < 400,
        "the frame's timers waited {ticked} ms for the program"
    );
}

#[test]
fn exec_is_refused_what_the_manifest_does_not_name_and_a_folder_outside_the_projects() {
    let rig = exec_rig(&["echo", "git status"]);
    let in_folder = |program: &str, args: &[&str], cwd: &str| {
        let mut params = exec(program, args);
        params["cwd"] = json!(cwd);
        params
    };
    assert_eq!(
        rig.refuses("exec", in_folder("git", &["log"], tmp().to_str().unwrap())),
        "probe may not run git log"
    );
    assert_eq!(
        rig.refuses("exec", exec("ls", &[])),
        "probe may not run ls "
    );
    // git always needs a folder, and any folder must be a project's.
    assert_eq!(
        rig.refuses("exec", exec("git", &["status"])),
        "git runs in a folder inside one of your Wings projects: pass its cwd"
    );
    let outside = rig.refuses("exec", in_folder("echo", &[], "/"));
    assert!(
        outside.contains("isn't inside any of your Wings projects"),
        "{outside}"
    );
    // The folder is checked first, as the desktop does, so a program that isn't allowed there says so about the folder.
    assert!(
        rig.refuses("exec", in_folder("rm", &[], "/"))
            .contains("isn't inside")
    );
}

#[test]
fn exec_takes_only_whole_millisecond_timeouts_and_says_when_a_program_is_not_installed() {
    let rig = exec_rig(&["echo", "wings-no-such-program-x"]);
    let timeout = |ms: Value| {
        let mut params = exec("echo", &[]);
        params["timeoutMs"] = ms;
        params
    };
    assert_eq!(
        rig.refuses("exec", timeout(json!(-1))),
        "exec timeoutMs must be a whole number of milliseconds, not -1"
    );
    assert_eq!(
        rig.refuses("exec", timeout(json!(1500.5))),
        "exec timeoutMs must be a whole number of milliseconds, not 1500.5"
    );
    assert_eq!(
        rig.refuses("exec", exec("wings-no-such-program-x", &[])),
        "wings-no-such-program-x is not installed or not on PATH"
    );
}

#[test]
fn exec_in_a_folder_of_a_pane_that_is_in_a_repo_is_allowed_like_the_desktop() {
    // A folder outside every project is allowed when a pane is in the same git repo: here the pane's folder is the repo.
    let repo = tmp().join(format!("wings-native-calls-repo-{}", std::process::id()));
    let inner = repo.join("src");
    fs::create_dir_all(&inner).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    let rig = exec_rig(&["echo"]);
    *rig.env.projects.lock().unwrap() = vec![];
    rig.host
        .publish_panes(vec![pane("1", repo.to_str().unwrap())]);
    let mut params = exec("echo", &["x"]);
    params["cwd"] = json!(inner);
    assert_eq!(rig.allows("exec", params)["stdout"], "x\n");
    let _ = fs::remove_dir_all(&repo);
}

// openUrl

#[test]
fn open_url_opens_only_what_the_manifest_names() {
    let rig = Rig::new(
        json!({ "permissions": { "openUrl": ["https://github.com/", "http://localhost:"] } }),
        panes(),
    );
    let open = |url: &str| json!({ "url": url });
    rig.allows("openUrl", open("https://github.com/a/b/pull/1"));
    rig.allows("openUrl", open("http://localhost:5173/"));
    assert_eq!(
        rig.env.seen(),
        [
            Seen::Url("https://github.com/a/b/pull/1".into()),
            Seen::Url("http://localhost:5173/".into())
        ]
    );
    for url in [
        "https://evil.example/",
        "file:///etc/passwd",
        "http://localhost.evil.com:80/",
        "http://localhost:80@evil.com/",
        "http://github.com/",
    ] {
        assert_eq!(
            rig.refuses("openUrl", open(url)),
            format!("probe may not open {url}")
        );
    }
    // No url is the text `undefined`, which names nothing.
    assert_eq!(
        rig.refuses("openUrl", json!({})),
        "probe may not open undefined"
    );
    assert_eq!(rig.env.seen().len(), 2);

    *rig.env.refuse.lock().unwrap() = Some("The browser didn't open".into());
    assert_eq!(
        rig.refuses("openUrl", open("https://github.com/")),
        "The browser didn't open"
    );
}

// ports

fn node(pane: &str, port: u16) -> PanePort {
    PanePort {
        pane_id: pane.into(),
        pid: 4242,
        name: "node".into(),
        port,
        started_at: 1_700_000_000_000,
    }
}

#[test]
fn ports_lists_what_the_app_found_with_the_permission_and_says_why_it_could_not() {
    let rig = Rig::new(json!({ "permissions": { "ports": true } }), panes());
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 5173), node("2", 8080)]);
    assert_eq!(
        rig.allows("ports", json!({})),
        json!([
            { "paneId": "1", "pid": 4242, "name": "node", "port": 5173, "startedAt": 1_700_000_000_000u64 },
            { "paneId": "2", "pid": 4242, "name": "node", "port": 8080, "startedAt": 1_700_000_000_000u64 },
        ])
    );
    *rig.env.ports.lock().unwrap() = Err("lsof isn't available: not found".into());
    assert_eq!(
        rig.refuses("ports", json!({})),
        "lsof isn't available: not found"
    );

    let none = Rig::new(
        json!({ "permissions": { "exec": ["lsof"], "statusline": true } }),
        panes(),
    );
    assert_eq!(
        none.refuses("ports", json!({})),
        "probe may not read listening ports"
    );
}

// notify

fn notifier() -> Rig {
    let rig = Rig::new(
        json!({ "name": "Build bot", "permissions": { "notify": true } }),
        panes(),
    );
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    rig
}

#[test]
fn a_notification_leads_with_the_plugins_name_is_cut_to_length_and_needs_its_permission() {
    let rig = notifier();
    let notify = |params: Value| rig.allows("notify", params);
    notify(json!({ "title": " Build passed ", "body": "main is green" }));
    notify(json!({ "title": "t".repeat(100), "body": "b".repeat(400), "projectId": tmp().to_str().unwrap() }));
    notify(json!({ "title": "No body", "body": "" }));
    let seen = rig.env.seen();
    assert_eq!(
        seen[0],
        Seen::Notice("Build bot: Build passed".into(), "main is green".into())
    );
    let Seen::Notice(title, body) = &seen[1] else {
        panic!("{seen:?}")
    };
    assert_eq!(
        (title.chars().count(), body.chars().count()),
        ("Build bot: ".len() + 64, 256)
    );
    assert!(body.ends_with('…'));
    assert_eq!(
        seen[2],
        Seen::Notice("Build bot: No body".into(), String::new())
    );

    let quiet = Rig::new(json!({ "name": "Quiet" }), panes());
    assert_eq!(
        quiet.refuses("notify", json!({ "title": "x", "body": "" })),
        "probe may not show notifications"
    );
    assert!(quiet.env.seen().is_empty());
}

#[test]
fn a_notification_needs_a_title_and_a_real_project_and_the_app_can_refuse_it() {
    let rig = notifier();
    for params in [
        json!({ "title": "  ", "body": "" }),
        json!({ "body": "no title" }),
        json!({ "body": "" }),
    ] {
        assert_eq!(
            rig.refuses("notify", params),
            "A notification needs a title"
        );
    }
    assert_eq!(
        rig.refuses("notify", json!({ "title": "x", "body": "", "projectId": "/not/a/project" })),
        "no project /not/a/project"
    );
    *rig.env.refuse.lock().unwrap() = Some("Notifications are off".into());
    assert_eq!(
        rig.refuses("notify", json!({ "title": "x", "body": "" })),
        "Notifications are off"
    );
    assert!(rig.env.seen().is_empty());
}

#[test]
fn a_notifications_buttons_keep_three_with_short_labels_and_open_only_tab_views_or_https_links() {
    use crate::page::plugin::NotifyButton;
    let plugin = crate::page::plugin::testing::plugin(json!({ "contributes": { "views": [
        { "id": "run", "title": "Run", "page": "run.html", "opens": ["tab"] },
        { "id": "pop", "title": "Pop", "page": "pop.html", "opens": ["modal"] },
    ] } }));
    let clean = |actions: serde_json::Value| super::notify_buttons(&plugin, Some(&actions));
    assert_eq!(
        clean(json!([
            { "label": format!("  {}  ", "l".repeat(40)), "url": "https://ci.example/1" },
            { "label": "Open", "open": { "view": "run", "args": { "id": 7 } } },
            { "label": "Plain", "open": { "view": "run" } },
            { "label": "Fourth", "url": "nothing checks this one" },
        ])),
        Ok(vec![
            NotifyButton::Url {
                label: "l".repeat(32),
                url: "https://ci.example/1".into()
            },
            NotifyButton::Open {
                label: "Open".into(),
                view: "run".into(),
                args: Some(json!({ "id": 7 }))
            },
            NotifyButton::Open {
                label: "Plain".into(),
                view: "run".into(),
                args: None
            },
        ])
    );
    assert_eq!(super::notify_buttons(&plugin, None), Ok(Vec::new()));
    assert_eq!(
        clean(json!({})),
        Err("notify actions must be a list".into())
    );
    assert_eq!(
        clean(json!([{ "url": "https://ci.example/" }])),
        Err("notify action 0 needs a label".into())
    );
    assert_eq!(
        clean(json!([{ "label": "Go", "url": "http://ci.example/" }])),
        Err("notify action Go needs an https link of up to 2048 characters".into())
    );
    assert_eq!(
        clean(json!([{ "label": "Go", "open": { "view": "pop" } }])),
        Err(
            "notify action Go opens pop, which isn't one of the plugin's views that open as a tab"
                .into()
        )
    );
    assert_eq!(
        clean(json!([{ "label": "Go", "open": { "view": "run", "args": "x".repeat(17 * 1024) } }])),
        Err("notify action args is limited to 16 KB".into())
    );
}

// copyText

#[test]
fn only_a_page_copies_and_only_right_after_a_click_or_key_and_never_more_than_a_megabyte() {
    const REFUSED: &str = "A page can copy only right after you click or type in it";
    let rig = Rig::new(json!({}), panes());
    let copy = |text: &str| json!({ "text": text });
    // Main has no clicks of yours, so it never copies.
    assert_eq!(
        call_from_main(&rig.host, "copyText", copy("from main")),
        Err(REFUSED.into())
    );
    // Nor does a page on its own, or on a hover, a blur or another event that isn't a gesture of yours. A click lets it
    // copy once, and never a megabyte and more.
    assert_eq!(
        call_at_start(&rig.host, "copyText", copy("unasked")),
        Err(REFUSED.into())
    );
    let page = Caller::start(&rig.host);
    assert_eq!(page.call("copyText", copy("hover")), Err(REFUSED.into()));
    assert_eq!(page.call_after_click("copyText", copy("fix/login")), Ok(Value::Null));
    // The click is used up by its copy.
    assert_eq!(page.call("copyText", copy("again")), Err(REFUSED.into()));
    assert_eq!(
        page.call_after_click("copyText", copy(&"x".repeat(1024 * 1024 + 1))),
        Err("copyText is limited to 1 MB of text".into())
    );
    assert_eq!(rig.env.seen(), [Seen::Copied("fix/login".into())]);
}

#[test]
fn notifications_are_rate_limited_per_plugin() {
    let rig = notifier();
    let page = Caller::start(&rig.host);
    let (mut sent, mut refused) = (0, None);
    for i in 0..22 {
        match page.call("notify", json!({ "title": format!("n{i}"), "body": "" })) {
            Ok(_) => sent += 1,
            Err(error) => refused = refused.or(Some(error)),
        }
    }
    assert_eq!(sent, 20);
    let refused = refused.unwrap();
    assert!(
        refused.starts_with("probe already showed 20 notifications this minute. Try again in ")
            && refused.ends_with(" s."),
        "{refused}"
    );
    assert_eq!(rig.env.seen().len(), 20);
}

// transcript and statusline

const SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";

fn transcripts(types: &[&str]) -> (Rig, std::path::PathBuf) {
    let rig = Rig::new(json!({ "permissions": { "transcript": types } }), panes());
    // Tests run in parallel and each removes its folder, so every call gets its own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let claude = tmp().join(format!(
        "wings-native-calls-claude-{}-{}-{}",
        std::process::id(),
        types.join("-"),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(claude.join("projects/-x")).unwrap();
    fs::write(
        claude.join(format!("projects/-x/{SESSION}.jsonl")),
        [
            r#"{"type":"user","n":1}"#,
            r#"{"type":"assistant","n":2,"message":{"content":[{"type":"user"}]}}"#,
            r#"{"type":"user","n":3}"#,
        ]
        .join("\n"),
    )
    .unwrap();
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    (rig, claude)
}

#[test]
fn transcript_gives_the_entries_of_the_types_the_manifest_names() {
    let (rig, claude) = transcripts(&["user"]);
    let read = |last: Value| {
        rig.allows("transcript", json!({ "sessionId": SESSION, "types": ["user"], "last": last }))
    };
    assert_eq!(
        read(Value::Null),
        json!([{ "type": "user", "n": 1 }, { "type": "user", "n": 3 }])
    );
    // `last` keeps the newest.
    assert_eq!(read(json!(1)), json!([{ "type": "user", "n": 3 }]));
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn transcript_is_refused_other_types_other_sessions_and_a_session_that_is_not_one() {
    let (rig, claude) = transcripts(&["assistant"]);
    let read = |session: &str, types: &[&str]| {
        rig.call("transcript", json!({ "sessionId": session, "types": types, "last": null }))
    };
    assert_eq!(
        read(SESSION, &["user"]),
        Err("probe may not read \"user\" transcript entries".into())
    );
    assert_eq!(
        read("../../etc/passwd", &["assistant"]),
        Err("not a session id".into())
    );
    assert_eq!(
        read("9d0e6a8e-1d1f-4c3e-8a43-5f3c1b2a7e10", &["assistant"]),
        Err("no transcript for that session".into())
    );
    // No types is nothing to read, so it is an empty list rather than a refusal.
    assert_eq!(read(SESSION, &[]), Ok(json!([])));
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn transcript_is_read_from_the_folder_of_any_account() {
    let (rig, claude) = transcripts(&["user"]);
    let default = claude.with_extension("default");
    fs::create_dir_all(default.join("projects/-x")).unwrap();
    let read = || rig.call("transcript", json!({ "sessionId": SESSION, "types": ["user"], "last": null }));
    // The session's transcript is in the second account's folder, not the first's.
    *rig.env.claude.lock().unwrap() = vec![default.clone(), claude.clone()];
    assert_eq!(read().unwrap().as_array().unwrap().len(), 2);
    *rig.env.claude.lock().unwrap() = vec![default.clone()];
    assert_eq!(read(), Err("no transcript for that session".into()));
    let _ = fs::remove_dir_all(claude);
    let _ = fs::remove_dir_all(default);
}

#[test]
fn a_transcript_entry_the_app_publishes_reaches_every_frame_and_a_listener_that_throws_stops_no_one()
 {
    let rig = Rig::new(
        json!({ "permissions": { "transcript": ["assistant"] } }),
        vec![],
    );
    let source = hears("transcript");
    let (first, second) = (
        Frame::start(&rig.host, &source),
        Frame::start(&rig.host, &source),
    );
    for frame in [&first, &second] {
        frame.report("the page to start", |_| true);
    }
    let entry =
        json!({ "sessionId": SESSION, "paneId": "1", "entry": { "type": "assistant", "n": 1 } });
    rig.host.publish_transcript(&entry.to_string());
    for frame in [&first, &second] {
        let report = frame.report("the entry", |r| {
            r["heard"].as_array().is_some_and(|h| !h.is_empty())
        });
        assert_eq!(report["heard"], json!([entry]));
    }
}

#[test]
fn statusline_has_what_claude_code_last_reported_and_needs_its_permission() {
    let rig = Rig::new(json!({ "permissions": { "statusline": true } }), panes());
    assert_eq!(
        rig.allows("statusline", json!({})),
        json!({ "rateLimits": [], "sessions": {} })
    );
    // What `wings statusline` handed the app.
    let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    rig.env.statusline.lock().unwrap().record(
        &json!({ "session_id": id, "rate_limits": { "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 } }, "context_window": { "used_percentage": 8 } }),
        &json!("work"),
        None,
        1000,
    );
    let status = rig.allows("statusline", json!({}));
    assert_eq!(
        status["rateLimits"],
        json!([{ "account": "work", "fiveHour": { "usedPercentage": 23.5, "resetsAt": 1738425600 }, "sevenDay": null, "at": 1000 }])
    );
    assert_eq!(status["sessions"][id]["usedPercentage"], 8.0);
    assert_eq!(status["sessions"][id]["account"], "work");
    let none = Rig::new(
        json!({ "permissions": { "transcript": ["assistant"] } }),
        panes(),
    );
    assert_eq!(
        none.refuses("statusline", json!({})),
        "probe may not read the status line"
    );
}

// claudeAccounts and setClaudeAccount

/// A plugin with or without `claudeAccounts`, in an app whose projects are `/a` and `/b`.
fn accounts(permitted: bool) -> (Rig, std::path::PathBuf) {
    let dir = crate::projects::test_dir("calls-accounts");
    let file = dir.join("spaces.json");
    let list = json!([
        { "id": "/a", "name": "a", "path": "/a" },
        { "id": "/b", "name": "b", "path": "/b" },
    ]);
    fs::write(&file, serde_json::to_vec(&list).unwrap()).unwrap();
    let rig = Rig::new(
        json!({ "permissions": { "claudeAccounts": permitted } }),
        panes(),
    );
    *rig.env.spaces.lock().unwrap() = crate::projects::Projects::open(Some(file), "/a".into());
    (rig, dir)
}

#[test]
fn a_project_is_given_an_account_and_claude_accounts_says_so() {
    use wings_spaces::claude_accounts::{claude_dir, dir};
    let (rig, folder) = accounts(true);
    let state = || rig.allows("claudeAccounts", json!({}));
    assert_eq!(state()["projects"], json!({}));
    // The default account leads, with the folder Wings runs under.
    assert_eq!(state()["accounts"][0]["name"], json!(null));
    assert_eq!(state()["accounts"][0]["dir"], json!(claude_dir()));

    // An assigned name is listed even before anyone signed in there, and the project is mapped to it by its id.
    let name = "wings-native-test";
    assert_eq!(
        rig.allows("setClaudeAccount", json!({ "projectId": "/a", "name": name })),
        json!(null)
    );
    let now = state();
    assert_eq!(now["projects"], json!({ "/a": name }));
    let listed = now["accounts"].as_array().unwrap();
    let account = listed.iter().find(|a| a["name"] == name).unwrap();
    assert_eq!(account["dir"], json!(dir(Some(name))));
    assert_eq!(account["signedIn"], false);
    assert_eq!(listed.iter().filter(|a| a["name"] == name).count(), 1);

    // null goes back to the default account.
    rig.allows("setClaudeAccount", json!({ "projectId": "/a", "name": null }));
    assert_eq!(state()["projects"], json!({}));

    // The desktop's words, in its order: the arguments, then the name, then the project.
    for (params, error) in [
        (
            json!({ "projectId": "", "name": null }),
            "setClaudeAccount needs a project id",
        ),
        (
            json!({ "projectId": 3, "name": "work" }),
            "setClaudeAccount needs a project id",
        ),
        (
            json!({ "projectId": "/a", "name": 3 }),
            "setClaudeAccount name is an account name, or null for the default account",
        ),
        (
            json!({ "projectId": "/a", "name": "Work" }),
            "\"Work\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        (
            json!({ "projectId": "/a", "name": "../x" }),
            "\"../x\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        (
            json!({ "projectId": "/nowhere", "name": "work" }),
            "No such project in Wings",
        ),
    ] {
        assert_eq!(rig.refuses("setClaudeAccount", params.clone()), error, "{params}");
    }
    assert_eq!(state()["projects"], json!({}));
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn claude_accounts_are_refused_to_a_plugin_without_their_permission() {
    let (rig, folder) = accounts(false);
    for (method, params) in [
        ("claudeAccounts", json!({})),
        ("setClaudeAccount", json!({ "projectId": "/a", "name": "work" })),
    ] {
        assert_eq!(
            rig.refuses(method, params),
            "probe may not use Claude Code accounts",
            "{method}"
        );
    }
    assert!(
        rig.env
            .spaces
            .lock()
            .unwrap()
            .claude_accounts(|_| Default::default())
            .projects
            .is_empty()
    );
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn share_claude_setup_turns_an_accounts_sharing_on_and_off_with_the_permission() {
    let (rig, folder) = accounts(true);
    for on in [true, false] {
        assert_eq!(
            rig.allows("shareClaudeSetup", json!({ "name": "work", "on": on })),
            json!(null)
        );
        assert_eq!(
            rig.env.seen().last(),
            Some(&Seen::SharedSetup("work".into(), on))
        );
    }
    let before = rig.env.seen().len();
    for (params, error) in [
        (
            json!({ "name": null, "on": true }),
            "shareClaudeSetup takes an account name and true or false",
        ),
        (
            json!({ "name": "../etc", "on": true }),
            "\"../etc\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        (
            json!({ "name": "Work", "on": false }),
            "\"Work\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
    ] {
        assert_eq!(rig.refuses("shareClaudeSetup", params.clone()), error, "{params}");
    }
    assert_eq!(rig.env.seen().len(), before, "nothing reached the folders");
    let (without, other) = accounts(false);
    assert_eq!(
        without.refuses("shareClaudeSetup", json!({ "name": "work", "on": true })),
        "probe may not use Claude Code accounts"
    );
    assert!(without.env.seen().is_empty());
    let _ = fs::remove_dir_all(folder);
    let _ = fs::remove_dir_all(other);
}

#[test]
fn open_pane_runs_under_the_account_asked_for_only_with_the_permission() {
    let open = |permissions: Value| {
        let rig = Rig::new(json!({ "permissions": permissions }), vec![]);
        let path = rig.root.config("probe").root.canonicalize().unwrap();
        *rig.env.projects.lock().unwrap() = vec![path];
        rig
    };
    let rig = open(json!({ "panes": ["claude"], "claudeAccounts": true }));
    rig.allows("openPane", open_pane(json!({ "command": "claude", "claudeAccount": "work" })));
    let seen = rig.env.seen();
    // The account is made ready before its terminal opens.
    assert_eq!(seen[0], Seen::PreparedAccount("work".into()));
    let Seen::OpenPane(plan) = &seen[1] else {
        panic!("no pane plan: {seen:?}")
    };
    assert_eq!(
        plan.claude_account,
        Some(AccountPick {
            name: Some("work".into())
        })
    );
    // null is the default account, whatever the project uses; left out, the project's own.
    rig.allows("openPane", open_pane(json!({ "command": "claude", "claudeAccount": null })));
    rig.allows("openPane", open_pane(json!({ "command": "claude" })));
    let plans: Vec<_> = rig
        .env
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::OpenPane(plan) => Some(plan.claude_account),
            _ => None,
        })
        .collect();
    assert_eq!(plans[1], Some(AccountPick { name: None }));
    assert_eq!(plans[2], None);
    for (account, error) in [
        (
            json!(5),
            "openPane claudeAccount is an account name, or null for the default account",
        ),
        (
            json!("../etc"),
            "\"../etc\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
    ] {
        assert_eq!(rig.refuses("openPane", open_pane(json!({ "claudeAccount": account }))), error);
    }
    // Without `claudeAccounts`, a pane may only run under its project's account.
    let plain = open(json!({ "panes": ["claude"] }));
    for account in [json!("work"), json!(null)] {
        assert_eq!(
            plain.refuses(
                "openPane",
                open_pane(json!({ "command": "claude", "claudeAccount": account }))
            ),
            "probe may not use Claude Code accounts"
        );
    }
    assert!(plain.env.seen().is_empty());
    plain.allows("openPane", open_pane(json!({ "command": "claude" })));
}

#[test]
fn a_sidebar_label_is_cleaned_and_only_for_the_plugins_own_pages() {
    let rig = Rig::new(
        json!({ "contributes": {
            "sidebars": [{ "id": "tracker", "title": "T", "page": "dist/s.html" }],
            "views": [{ "id": "board", "title": "B", "page": "dist/b.html", "opens": ["tab"] }],
        } }),
        vec![],
    );
    let label = |seen: Vec<Seen>| match seen.last() {
        Some(Seen::SidebarLabel(plugin, page, label)) => {
            (plugin.clone(), page.clone(), label.clone())
        }
        other => panic!("{other:?}"),
    };
    let mut rows = vec![json!({ "label": "Project", "value": "Wings" }), json!({ "label": "", "value": "x" })];
    rows.extend((0..8).map(|_| json!({ "label": "n", "value": "v" })));
    rig.allows(
        "setSidebarLabel",
        json!({ "sidebarId": "tracker", "label": "1:05:00 ".repeat(20), "tone": "danger", "rows": rows }),
    );
    let (plugin, page, shown) = label(rig.env.seen());
    assert_eq!((plugin.as_str(), page.as_str()), ("probe", "tracker"));
    let shown = shown.unwrap();
    assert_eq!(shown.text.encode_utf16().count(), 80);
    assert_eq!(shown.tone, Some(crate::page::plugin::ui::Tone::Danger));
    // Six rows at most, and one without a label is dropped.
    assert_eq!(shown.rows.len(), 5);
    assert_eq!(shown.rows[0], ("Project".into(), "Wings".into()));
    // A view's button takes one too, and a tone that isn't warning or danger is the live green dot.
    rig.allows("setSidebarLabel", json!({ "sidebarId": "board", "label": "3", "tone": "success" }));
    assert_eq!(label(rig.env.seen()).2.unwrap().tone, None);
    // null, or no text, clears it.
    for clear in [json!(null), json!("")] {
        rig.allows("setSidebarLabel", json!({ "sidebarId": "tracker", "label": "x" }));
        rig.allows("setSidebarLabel", json!({ "sidebarId": "tracker", "label": clear }));
        assert_eq!(label(rig.env.seen()).2, None, "{clear}");
    }
    assert_eq!(
        rig.refuses("setSidebarLabel", json!({ "sidebarId": "elsewhere", "label": "x" })),
        "probe has no sidebar or view elsewhere"
    );
}

#[test]
fn focus_pane_needs_panes_granted_and_a_pane_that_is_there() {
    let rig = Rig::new(json!({ "permissions": { "panes": [] } }), panes());
    assert_eq!(rig.allows("focusPane", json!({ "paneId": "2" })), json!(null));
    assert_eq!(rig.env.seen(), [Seen::FocusPane("2".into())]);
    assert_eq!(rig.refuses("focusPane", json!({ "paneId": "9" })), "no pane 9");
    let without = Rig::new(json!({}), panes());
    assert_eq!(
        without.refuses("focusPane", json!({ "paneId": "1" })),
        "probe may not focus panes"
    );
}

#[test]
fn a_frame_that_sends_the_wrong_json_types_is_refused_and_nothing_reaches_the_app() {
    // A WebAssembly frame sends raw JSON, where the script frames' wrapper made strings, lists and numbers of the arguments.
    let rig = Rig::new(
        json!({
            "permissions": { "exec": ["echo"], "openUrl": ["https://github.com/"], "panes": [], "transcript": ["user"], "agentActivity": true },
            "contributes": { "views": [{ "id": "history", "title": "History", "page": "history.html", "opens": ["tab"] }] },
        }),
        panes(),
    );
    *rig.env.projects.lock().unwrap() = vec![tmp()];
    let with = |mut params: Value, key: &str, value: Value| {
        params[key] = value;
        params
    };
    let without = |mut params: Value, key: &str| {
        params.as_object_mut().unwrap().remove(key);
        params
    };
    let echo = exec("echo", &["x"]);
    let read = json!({ "sessionId": SESSION, "types": ["user"], "last": null });
    let view = json!({ "id": "history", "newTab": false });
    let mut wrong = vec![("exec", json!({})), ("exec", json!([])), ("transcript", json!({}))];
    for (key, value) in [
        ("program", json!(5)),
        ("program", json!(null)),
        ("args", json!("x")),
        ("args", json!({})),
        ("args", json!([5])),
        ("cwd", json!(5)),
        ("timeoutMs", json!("soon")),
        ("stream", json!("yes")),
    ] {
        wrong.push(("exec", with(echo.clone(), key, value)));
    }
    for key in ["program", "args", "stream"] {
        wrong.push(("exec", without(echo.clone(), key)));
    }
    for (key, value) in [
        ("sessionId", json!(5)),
        ("sessionId", json!(null)),
        ("types", json!("user")),
        ("types", json!({})),
        ("types", json!([5])),
        ("types", json!(null)),
        ("last", json!("1")),
        ("last", json!({})),
    ] {
        wrong.push(("transcript", with(read.clone(), key, value)));
    }
    for key in ["sessionId", "types"] {
        wrong.push(("transcript", without(read.clone(), key)));
    }
    // `js_string` makes text of whatever it is given, so these name nothing the manifest or the app knows.
    for url in [json!(5), json!(null), json!({}), json!(true)] {
        wrong.push(("openUrl", json!({ "url": url })));
    }
    wrong.push(("openUrl", json!({})));
    for pane in [json!(null), json!({}), json!(true), json!(9)] {
        wrong.push(("focusPane", json!({ "paneId": pane })));
    }
    wrong.push(("focusPane", json!({})));
    for session in [json!(null), json!({}), json!(true)] {
        wrong.push(("agentActivity", json!({ "sessionId": session })));
    }
    wrong.push(("agentActivity", json!({})));
    for (key, value) in [("id", json!(5)), ("id", json!(null)), ("id", json!({})), ("as", json!(5)), ("newTab", json!("yes")), ("newTab", json!(null))] {
        wrong.push(("openView", with(view.clone(), key, value)));
    }
    wrong.push(("openView", json!({})));
    let page = Caller::start(&rig.host);
    for (method, params) in wrong {
        assert!(page.call(method, params.clone()).is_err(), "{method} {params}");
    }
    assert_eq!(rig.env.seen(), []);

    // What the host accepts by design: `js_string` takes any JSON for a text, as `String(value)` does, and the manifest and
    // the app are asked about that text. So a list of one URL is the URL, a number is its digits, and a missing text is
    // "undefined".
    assert_eq!(page.call("openUrl", json!({ "url": ["https://github.com/"] })), Ok(Value::Null));
    assert_eq!(page.call("focusPane", json!({ "paneId": 1 })), Ok(Value::Null));
    assert_eq!(page.call("focusPane", json!({ "paneId": ["1"] })), Ok(Value::Null));
    for text in [json!(5), json!(null), json!({}), json!(["x"]), json!(true)] {
        assert_eq!(page.call_after_click("copyText", json!({ "text": text })), Ok(Value::Null));
    }
    assert_eq!(page.call_after_click("copyText", json!({})), Ok(Value::Null));
    // A view's args are any JSON that fits 16 KB, and an exec's cwd and timeout are optional.
    assert_eq!(page.call("openView", with(view.clone(), "args", json!("x"))), Ok(Value::Null));
    for key in ["cwd", "timeoutMs"] {
        assert_eq!(page.call("exec", without(echo.clone(), key)).unwrap()["code"], 0);
    }
    assert_eq!(
        rig.env.seen(),
        [
            Seen::Url("https://github.com/".into()),
            Seen::FocusPane("1".into()),
            Seen::FocusPane("1".into()),
            Seen::Copied("5".into()),
            Seen::Copied("null".into()),
            Seen::Copied("[object Object]".into()),
            Seen::Copied("x".into()),
            Seen::Copied("true".into()),
            Seen::Copied("undefined".into()),
            Seen::OpenView(crate::page::plugin::OpenView {
                plugin: "probe".into(),
                id: "history".into(),
                place: "tab".into(),
                args: Some(json!("x")),
                new_tab: false,
            }),
        ]
    );
    // The app's lookup of a session is asked for that text too, and says it knows none of them.
    assert_eq!(rig.env.asked(), ["null", "[object Object]", "true", "undefined"]);
}
