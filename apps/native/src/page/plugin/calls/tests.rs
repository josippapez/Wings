//! Each `wings.*` call of a native frame: what it does when the manifest grants it, what it says when it doesn't, and
//! what it says of bad arguments. The words are the desktop's.

use std::{fs, sync::mpsc, time::Duration};

use serde_json::{Value, json};
use wings_spaces::claude_accounts::AccountPick;

use crate::page::plugin::{
    PanePort, PluginHost,
    testing::{Frame, PRELUDE, Rig, Seen, WAIT, pane},
    tests::Root,
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
    for call in [
        "wings.exec('echo', [])",
        "wings.setBadge('1', { label: 'x' })",
        "wings.openUrl('https://github.com/')",
        "wings.ports()",
        "wings.statusline()",
        "wings.notify({ title: 'x' })",
        "wings.transcript('3c21b0c5-8113-4f55-9493-fa156c3fa369', ['user'])",
    ] {
        assert_eq!(
            rig.refused(&format!("return await {call};")),
            "stray is turned off or not installed",
            "{call}"
        );
    }
}

// onPanes

#[test]
fn every_frame_hears_the_panes_as_the_desktop_lists_them_when_it_starts_and_when_they_change() {
    let rig = Rig::new(json!({}), panes());
    let frame = Frame::start(
        &rig.host,
        &format!(
            "{PRELUDE}
            const heard = [];
            wings.onPanes((list) => {{ heard.push(list); show({{ heard }}); }});"
        ),
    );
    // The list that was there before the frame started comes once its bundle has run.
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

#[test]
fn a_listener_that_comes_late_hears_the_last_list_and_one_that_is_removed_hears_no_more() {
    let rig = Rig::new(json!({}), panes());
    let result = rig.allowed(
        "const first = []; const late = [];
         const off = wings.onPanes((list) => first.push(list.length));
         await new Promise((resolve) => setTimeout(resolve, 50));
         wings.onPanes((list) => late.push(list.length));
         off();
         await new Promise((resolve) => setTimeout(resolve, 20));
         return { first, late };",
    );
    assert_eq!(result, json!({ "first": [2], "late": [2] }));
}

#[test]
fn a_listener_that_throws_does_not_stop_the_others() {
    let rig = Rig::new(json!({}), panes());
    let result = rig.allowed(
        "const heard = [];
         wings.onPanes(() => { throw new Error('boom'); });
         wings.onPanes((list) => heard.push(list.length));
         await new Promise((resolve) => setTimeout(resolve, 50));
         return heard;",
    );
    assert_eq!(result, json!([2]));
}

// setBadge

fn badges() -> Rig {
    Rig::new(json!({ "contributes": { "ui": ["badges"] } }), panes())
}

#[test]
fn a_badge_is_cleaned_and_handed_to_the_app_and_a_repeat_is_not() {
    let rig = badges();
    let result = rig.allowed(
        "const badge = { label: 'localhost:5173', tone: 'success', actions: [{ id: 'open', label: 'Open', primary: true }], extra: 1 };
         await wings.setBadge('1', badge);
         await wings.setBadge('1', badge);
         await wings.setBadge('1', null);
         await wings.setBadge('1', null);
         return 'done';",
    );
    assert_eq!(result, "done");
    let seen = rig.env.seen();
    // Sending what the frame last sent for the pane does nothing, `null` included.
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
    rig.allowed("await wings.setBadge('2', { tone: 'info' }); return 1;");
    assert_eq!(
        rig.env.seen(),
        [Seen::Badge("probe".into(), "2".into(), None)]
    );
    // A pane that isn't there is refused, and asking again asks again.
    assert_eq!(
        rig.refused("try { await wings.setBadge('9', { label: 'x' }); } catch {} await wings.setBadge('9', { label: 'x' });"),
        "no pane 9"
    );
}

#[test]
fn a_badge_needs_badges_declared_and_a_pane_that_is_there() {
    let rig = Rig::new(json!({ "contributes": { "ui": ["status"] } }), panes());
    assert_eq!(
        rig.refused("await wings.setBadge('1', { label: 'x' });"),
        "probe must declare \"badges\" in contributes.ui"
    );
    assert_eq!(
        badges().refused("await wings.setBadge('7', { label: 'x' });"),
        "no pane 7"
    );
    assert!(rig.env.seen().is_empty());
}

// onAction

const MAIN: &str = "
wings.onAction(async ({ paneId, actionId }) => {
  await wings.storage.set('last', { paneId, actionId });
  if (actionId === 'boom') throw new Error('It broke');
  if (actionId === 'slow') await new Promise((resolve) => setTimeout(resolve, 100));
});
wings.onAction(async ({ actionId }) => { if (actionId === 'slow') await wings.storage.set('second', 'done'); });
";

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
    rig.host.start_main(MAIN, |_| {}).unwrap();
    assert_eq!(action(&rig.host, "1", "open"), Ok(()));
    assert_eq!(
        rig.root.saved("probe", "last"),
        Some(json!({ "paneId": "1", "actionId": "open" }))
    );
    // The spinner stays until every listener is done, not just the first.
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

    rig.host
        .start_main("wings.onAction(() => new Promise(() => {}));", |_| {})
        .unwrap();
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
    rig.allowed(
        "const status = { items: [{ label: '5h', value: '23%', percent: 23, tone: 'warning' }], rows: [{ label: 'Resets', value: 'in 2h' }], open: 'hud' };
         await wings.setPaneStatus('1', status);
         await wings.setPaneStatus('1', status);
         await wings.setPaneStatus('1', { items: [{ value: 'x' }], open: 'board' });
         await wings.setPaneStatus('1', null);
         return 1;",
    );
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
    let refused = |body: &str| rig.refused(body);
    assert_eq!(
        refused("await wings.setPaneStatus('8', { items: [{ value: 'x' }] });"),
        "no pane 8"
    );
    assert_eq!(
        refused("await wings.setPaneStatus('1', { items: [] });"),
        "setPaneStatus: items must be a non-empty array"
    );
    assert_eq!(
        refused("await wings.setPaneStatus('1', { items: [{ value: 'x'.repeat(33) }] });"),
        "setPaneStatus: items[0].value is over 32 characters"
    );
    assert_eq!(
        refused("await wings.setPaneStatus('1', { items: [{ value: 'x', percent: 120 }] });"),
        "setPaneStatus: items[0].percent must be a number from 0 to 100"
    );
    assert_eq!(
        refused("await wings.setPaneStatus('1', { items: [{ value: 'x' }], open: 'nope' });"),
        "setPaneStatus: open must be the id of one of the plugin's sidebars or views (hud, board)"
    );
    // Nothing at all is not a status, though `null` is the way to remove one.
    assert_eq!(
        refused(
            "await wings.setPaneStatus('1', { items: [{ value: 'x' }] }); await wings.setPaneStatus('1');"
        ),
        "setPaneStatus: status must be an object or null"
    );
    assert_eq!(rig.env.seen().len(), 1);

    let undeclared = Rig::new(json!({ "contributes": { "ui": ["badges"] } }), panes());
    assert_eq!(
        undeclared.refused("await wings.setPaneStatus('1', { items: [{ value: 'x' }] });"),
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
        rig.allowed("return await wings.exec('echo', ['hi', 3]);"),
        json!({ "code": 0, "stdout": "hi 3\n", "stderr": "" })
    );
    // Arguments that aren't a list are no arguments, and the options are optional.
    assert_eq!(
        rig.allowed("return (await wings.exec('echo', 'x')).stdout;"),
        "\n"
    );
    assert_eq!(
        rig.allowed(&format!(
            "return (await wings.exec('echo', ['in'], {{ cwd: {:?}, timeoutMs: 5000 }})).code;",
            tmp()
        )),
        0
    );
}

#[test]
fn exec_streams_each_line_while_it_runs_when_asked() {
    let rig = exec_rig(&["echo"]);
    assert_eq!(
        rig.allowed("const lines = []; const r = await wings.exec('echo', ['one'], { onOutput: (line) => lines.push(line) }); return { lines, stdout: r.stdout };"),
        json!({ "lines": ["one"], "stdout": "one\n" })
    );
}

#[test]
fn exec_does_not_hold_the_frame_up_while_the_program_runs() {
    let rig = exec_rig(&["sleep"]);
    let result = rig.allowed(
        "const slow = wings.exec('sleep', ['0.6']);
         const started = Date.now();
         let ticks = 0;
         await new Promise((resolve) => { const id = setInterval(() => { if (++ticks === 3) { clearInterval(id); resolve(); } }, 20); });
         const ticked = Date.now() - started;
         return { ticked, code: (await slow).code };",
    );
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
    let cwd = format!("{:?}", tmp());
    assert_eq!(
        rig.refused(&format!(
            "await wings.exec('git', ['log'], {{ cwd: {cwd} }});"
        )),
        "probe may not run git log"
    );
    assert_eq!(
        rig.refused("await wings.exec('ls', []);"),
        "probe may not run ls "
    );
    // git always needs a folder, and any folder must be a project's.
    assert_eq!(
        rig.refused("await wings.exec('git', ['status']);"),
        "git runs in a folder inside one of your Wings projects: pass its cwd"
    );
    let outside = rig.refused("await wings.exec('echo', [], { cwd: '/' });");
    assert!(
        outside.contains("isn't inside any of your Wings projects"),
        "{outside}"
    );
    // The folder is checked first, as the desktop does, so a program that isn't allowed there says so about the folder.
    assert!(
        rig.refused("await wings.exec('rm', [], { cwd: '/' });")
            .contains("isn't inside")
    );
}

#[test]
fn exec_takes_only_whole_millisecond_timeouts_and_says_when_a_program_is_not_installed() {
    let rig = exec_rig(&["echo", "wings-no-such-program-x"]);
    assert_eq!(
        rig.refused("await wings.exec('echo', [], { timeoutMs: -1 });"),
        "exec timeoutMs must be a whole number of milliseconds, not -1"
    );
    assert_eq!(
        rig.refused("await wings.exec('echo', [], { timeoutMs: 1500.5 });"),
        "exec timeoutMs must be a whole number of milliseconds, not 1500.5"
    );
    // Not a number at all is no timeout, as in the desktop's wrapper.
    assert_eq!(
        rig.allowed("return (await wings.exec('echo', [], { timeoutMs: 'soon' })).code;"),
        0
    );
    assert_eq!(
        rig.refused("await wings.exec('wings-no-such-program-x', []);"),
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
    assert_eq!(
        rig.allowed(&format!(
            "return (await wings.exec('echo', ['x'], {{ cwd: {:?} }})).stdout;",
            inner
        )),
        "x\n"
    );
    let _ = fs::remove_dir_all(&repo);
}

// openUrl

#[test]
fn open_url_opens_only_what_the_manifest_names() {
    let rig = Rig::new(
        json!({ "permissions": { "openUrl": ["https://github.com/", "http://localhost:"] } }),
        panes(),
    );
    rig.allowed("await wings.openUrl('https://github.com/a/b/pull/1'); await wings.openUrl('http://localhost:5173/');");
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
            rig.refused(&format!("await wings.openUrl({url:?});")),
            format!("probe may not open {url}")
        );
    }
    // No argument is the text `undefined`, which names nothing.
    assert_eq!(
        rig.refused("await wings.openUrl();"),
        "probe may not open undefined"
    );
    assert_eq!(rig.env.seen().len(), 2);

    *rig.env.refuse.lock().unwrap() = Some("The browser didn't open".into());
    assert_eq!(
        rig.refused("await wings.openUrl('https://github.com/');"),
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
        rig.allowed("return await wings.ports();"),
        json!([
            { "paneId": "1", "pid": 4242, "name": "node", "port": 5173, "startedAt": 1_700_000_000_000u64 },
            { "paneId": "2", "pid": 4242, "name": "node", "port": 8080, "startedAt": 1_700_000_000_000u64 },
        ])
    );
    *rig.env.ports.lock().unwrap() = Err("lsof isn't available: not found".into());
    assert_eq!(
        rig.refused("await wings.ports();"),
        "lsof isn't available: not found"
    );

    let none = Rig::new(
        json!({ "permissions": { "exec": ["lsof"], "statusline": true } }),
        panes(),
    );
    assert_eq!(
        none.refused("await wings.ports();"),
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
    rig.allowed(&format!(
        "await wings.notify({{ title: ' Build passed ', body: 'main is green' }});
         await wings.notify({{ title: 't'.repeat(100), body: 'b'.repeat(400), projectId: {:?} }});
         await wings.notify({{ title: 'No body' }});",
        tmp().to_str().unwrap()
    ));
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
        quiet.refused("await wings.notify({ title: 'x' });"),
        "probe may not show notifications"
    );
    assert!(quiet.env.seen().is_empty());
}

#[test]
fn a_notification_needs_a_title_and_a_real_project_and_the_app_can_refuse_it() {
    let rig = notifier();
    assert_eq!(
        rig.refused("await wings.notify({ title: '  ' });"),
        "A notification needs a title"
    );
    assert_eq!(
        rig.refused("await wings.notify({ body: 'no title' });"),
        "A notification needs a title"
    );
    assert_eq!(
        rig.refused("await wings.notify();"),
        "A notification needs a title"
    );
    assert_eq!(
        rig.refused("await wings.notify({ title: 'x', projectId: '/not/a/project' });"),
        "no project /not/a/project"
    );
    *rig.env.refuse.lock().unwrap() = Some("Notifications are off".into());
    assert_eq!(
        rig.refused("await wings.notify({ title: 'x' });"),
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
    // Main has no clicks of yours, so it never copies.
    rig.host
        .start_main(
            "wings.copyText('from main').then(() => 'copied', (e) => e.message).then((r) => wings.storage.set('copy', r));",
            |_| {},
        )
        .unwrap();
    // Nor does a page on its own, or on a hover, a blur or another event that isn't a gesture of yours. A click lets it
    // copy once, and never a megabyte and more.
    let page = Frame::start(
        &rig.host,
        "const show = (value) => wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
           { id: 1, type: '#text', text: JSON.stringify(value) }] }));
         const copy = (handler) => wings.copyText(handler === 'big' ? 'x'.repeat(1024 * 1024 + 1) : 'fix/login')
           .then(() => show({ handler, result: 'copied' }), (error) => show({ handler, result: error.message }));
         copy('unasked');
         globalThis.__wingsEvent = (handler) => { copy(handler); return true; };",
    );
    let result = |handler: &str| {
        page.report(handler, |r| r["handler"] == handler)["result"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(result("unasked"), REFUSED);
    assert!(page.dispatch("hover"));
    assert_eq!(result("hover"), REFUSED);
    assert!(page.click("click"));
    assert_eq!(result("click"), "copied");
    // The click is used up by its copy.
    assert!(page.dispatch("again"));
    assert_eq!(result("again"), REFUSED);
    assert!(page.click("big"));
    assert_eq!(result("big"), "copyText is limited to 1 MB of text");
    let deadline = std::time::Instant::now() + WAIT;
    while rig.root.saved("probe", "copy").is_none() {
        assert!(std::time::Instant::now() < deadline, "main never settled");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig.root.saved("probe", "copy"), Some(json!(REFUSED)));
    assert_eq!(rig.env.seen(), [Seen::Copied("fix/login".into())]);
}

#[test]
fn notifications_are_rate_limited_per_plugin() {
    let rig = notifier();
    let result = rig.allowed(
        "let sent = 0; let refused = null;
         for (let i = 0; i < 22; i++) {
           try { await wings.notify({ title: `n${i}` }); sent++; } catch (error) { refused ??= error.message; }
         }
         return { sent, refused };",
    );
    assert_eq!(result["sent"], 20);
    let refused = result["refused"].as_str().unwrap();
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
    let entries = rig.allowed(&format!(
        "return await wings.transcript('{SESSION}', ['user']);"
    ));
    assert_eq!(
        entries,
        json!([{ "type": "user", "n": 1 }, { "type": "user", "n": 3 }])
    );
    // `last` keeps the newest, as a whole number from 1; anything else means all of them.
    assert_eq!(
        rig.allowed(&format!(
            "return await wings.transcript('{SESSION}', ['user'], {{ last: 1.9 }});"
        )),
        json!([{ "type": "user", "n": 3 }])
    );
    assert_eq!(
        rig.allowed(&format!(
            "return (await wings.transcript('{SESSION}', ['user'], {{ last: 0 }})).length;"
        )),
        2
    );
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn transcript_is_refused_other_types_other_sessions_and_a_session_that_is_not_one() {
    let (rig, claude) = transcripts(&["assistant"]);
    assert_eq!(
        rig.refused(&format!("await wings.transcript('{SESSION}', ['user']);")),
        "probe may not read \"user\" transcript entries"
    );
    assert_eq!(
        rig.refused("await wings.transcript('../../etc/passwd', ['assistant']);"),
        "not a session id"
    );
    assert_eq!(
        rig.refused(
            "await wings.transcript('9d0e6a8e-1d1f-4c3e-8a43-5f3c1b2a7e10', ['assistant']);"
        ),
        "no transcript for that session"
    );
    // No types is nothing to read, so it is an empty list rather than a refusal.
    assert_eq!(
        rig.allowed(&format!("return await wings.transcript('{SESSION}');")),
        json!([])
    );
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn transcript_is_read_from_the_folder_of_any_account() {
    let (rig, claude) = transcripts(&["user"]);
    let default = claude.with_extension("default");
    fs::create_dir_all(default.join("projects/-x")).unwrap();
    let read = format!("return (await wings.transcript('{SESSION}', ['user'])).length;");
    // The session's transcript is in the second account's folder, not the first's.
    *rig.env.claude.lock().unwrap() = vec![default.clone(), claude.clone()];
    assert_eq!(rig.allowed(&read), 2);
    *rig.env.claude.lock().unwrap() = vec![default.clone()];
    assert_eq!(rig.refused(&read), "no transcript for that session");
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
    let source = format!(
        "{PRELUDE}
         const heard = [];
         wings.onTranscript(() => {{ throw new Error('boom'); }});
         wings.onTranscript((event) => {{ heard.push(event); show({{ heard }}); }});
         show({{ heard }});"
    );
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
        rig.allowed("return await wings.statusline();"),
        json!({ "rateLimits": null, "sessions": {} })
    );
    // What `wings statusline` handed the app.
    let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    rig.env.statusline.lock().unwrap().record(
        &json!({ "session_id": id, "rate_limits": { "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 } }, "context_window": { "used_percentage": 8 } }),
        1000,
    );
    let status = rig.allowed("return await wings.statusline();");
    assert_eq!(
        status["rateLimits"],
        json!({ "fiveHour": { "usedPercentage": 23.5, "resetsAt": 1738425600 }, "sevenDay": null, "at": 1000 })
    );
    assert_eq!(status["sessions"][id]["usedPercentage"], 8.0);
    let none = Rig::new(
        json!({ "permissions": { "transcript": ["assistant"] } }),
        panes(),
    );
    assert_eq!(
        none.refused("await wings.statusline();"),
        "probe may not read the status line"
    );
}

#[test]
fn the_calls_are_on_a_frame_of_a_plugin_and_frozen_with_it() {
    let rig = Rig::new(json!({}), vec![]);
    let names = rig.allowed(
        "const kinds = Object.fromEntries(['onPanes', 'setBadge', 'setPaneStatus', 'onAction', 'exec', 'openUrl', 'ports', 'notify', 'transcript', 'onTranscript', 'statusline'].map((name) => [name, typeof wings[name]]));
         try { wings.exec = () => {}; } catch {}
         return { kinds, frozen: Object.isFrozen(wings) };",
    );
    assert_eq!(names["frozen"], true);
    for (name, kind) in names["kinds"].as_object().unwrap() {
        assert_eq!(kind, "function", "{name}");
    }
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
    let state = || rig.allowed("return await wings.claudeAccounts();");
    assert_eq!(state()["projects"], json!({}));
    // The default account leads, with the folder Wings runs under.
    assert_eq!(state()["accounts"][0]["name"], json!(null));
    assert_eq!(state()["accounts"][0]["dir"], json!(claude_dir()));

    // An assigned name is listed even before anyone signed in there, and the project is mapped to it by its id.
    let name = "wings-native-test";
    assert_eq!(
        rig.allowed(&format!(
            "return await wings.setClaudeAccount('/a', '{name}');"
        )),
        json!(null)
    );
    let now = state();
    assert_eq!(now["projects"], json!({ "/a": name }));
    let listed = now["accounts"].as_array().unwrap();
    let account = listed.iter().find(|a| a["name"] == name).unwrap();
    assert_eq!(account["dir"], json!(dir(Some(name))));
    assert_eq!(account["signedIn"], false);
    assert_eq!(listed.iter().filter(|a| a["name"] == name).count(), 1);

    // No name, or null, goes back to the default account.
    for call in [
        "wings.setClaudeAccount('/a')",
        "wings.setClaudeAccount('/a', null)",
    ] {
        rig.allowed(&format!(
            "await wings.setClaudeAccount('/a', '{name}'); return await {call};"
        ));
        assert_eq!(state()["projects"], json!({}), "{call}");
    }

    // The desktop's words, in its order: the arguments, then the name, then the project.
    let set = |args: &str| format!("return await wings.setClaudeAccount({args});");
    for (args, error) in [
        ("''", "setClaudeAccount needs a project id"),
        ("3, 'work'", "setClaudeAccount needs a project id"),
        (
            "'/a', 3",
            "setClaudeAccount name is an account name, or null for the default account",
        ),
        (
            "'/a', 'Work'",
            "\"Work\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        (
            "'/a', '../x'",
            "\"../x\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        ("'/nowhere', 'work'", "No such project in Wings"),
    ] {
        assert_eq!(rig.refused(&set(args)), error, "{args}");
    }
    assert_eq!(state()["projects"], json!({}));
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn claude_accounts_are_refused_to_a_plugin_without_their_permission() {
    let (rig, folder) = accounts(false);
    for call in [
        "wings.claudeAccounts()",
        "wings.setClaudeAccount('/a', 'work')",
    ] {
        assert_eq!(
            rig.refused(&format!("return await {call};")),
            "probe may not use Claude Code accounts",
            "{call}"
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
    for (on, js) in [(true, "true"), (false, "false")] {
        assert_eq!(
            rig.allowed(&format!(
                "return await wings.shareClaudeSetup('work', {js});"
            )),
            json!(null)
        );
        assert_eq!(
            rig.env.seen().last(),
            Some(&Seen::SharedSetup("work".into(), on))
        );
    }
    // As the desktop's frame does: anything but `true` is off.
    rig.allowed("return await wings.shareClaudeSetup('work', 'yes');");
    assert_eq!(
        rig.env.seen().last(),
        Some(&Seen::SharedSetup("work".into(), false))
    );
    let before = rig.env.seen().len();
    for (args, error) in [
        (
            "null, true",
            "shareClaudeSetup takes an account name and true or false",
        ),
        (
            "'../etc', true",
            "\"../etc\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
        (
            "'Work', false",
            "\"Work\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
    ] {
        assert_eq!(
            rig.refused(&format!("return await wings.shareClaudeSetup({args});")),
            error,
            "{args}"
        );
    }
    assert_eq!(rig.env.seen().len(), before, "nothing reached the folders");
    let (without, other) = accounts(false);
    assert_eq!(
        without.refused("return await wings.shareClaudeSetup('work', true);"),
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
    rig.allowed("return await wings.openPane({ command: 'claude', claudeAccount: 'work' });");
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
    rig.allowed("return await wings.openPane({ command: 'claude', claudeAccount: null });");
    rig.allowed("return await wings.openPane({ command: 'claude' });");
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
    for (body, error) in [
        (
            "wings.openPane({ claudeAccount: 5 })",
            "openPane claudeAccount is an account name, or null for the default account",
        ),
        (
            "wings.openPane({ claudeAccount: '../etc' })",
            "\"../etc\" isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit",
        ),
    ] {
        assert_eq!(rig.refused(&format!("return await {body};")), error);
    }
    // Without `claudeAccounts`, a pane may only run under its project's account.
    let plain = open(json!({ "panes": ["claude"] }));
    for account in ["'work'", "null"] {
        assert_eq!(
            plain.refused(&format!(
                "return await wings.openPane({{ command: 'claude', claudeAccount: {account} }});"
            )),
            "probe may not use Claude Code accounts"
        );
    }
    assert!(plain.env.seen().is_empty());
    plain.allowed("return await wings.openPane({ command: 'claude' });");
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
    rig.allowed(&format!(
        "return await wings.setSidebarLabel('tracker', '{}', {{ tone: 'danger', rows: [{{ label: 'Project', value: 'Wings' }}, {{ label: '', value: 'x' }}, ...Array(8).fill({{ label: 'n', value: 'v' }})] }});",
        "1:05:00 ".repeat(20)
    ));
    let (plugin, page, shown) = label(rig.env.seen());
    assert_eq!((plugin.as_str(), page.as_str()), ("probe", "tracker"));
    let shown = shown.unwrap();
    assert_eq!(shown.text.encode_utf16().count(), 80);
    assert_eq!(shown.tone, Some(crate::page::plugin::ui::Tone::Danger));
    // Six rows at most, and one without a label is dropped.
    assert_eq!(shown.rows.len(), 5);
    assert_eq!(shown.rows[0], ("Project".into(), "Wings".into()));
    // A view's button takes one too, and a tone that isn't warning or danger is the live green dot.
    rig.allowed("return await wings.setSidebarLabel('board', '3', { tone: 'success' });");
    assert_eq!(label(rig.env.seen()).2.unwrap().tone, None);
    // null, or no text, clears it.
    for clear in ["null", "''"] {
        rig.allowed(&format!(
            "await wings.setSidebarLabel('tracker', 'x'); return await wings.setSidebarLabel('tracker', {clear});"
        ));
        assert_eq!(label(rig.env.seen()).2, None, "{clear}");
    }
    assert_eq!(
        rig.refused("return await wings.setSidebarLabel('elsewhere', 'x');"),
        "probe has no sidebar or view elsewhere"
    );
    // The same label again is never sent.
    let before = rig.env.seen().len();
    rig.allowed("await wings.setSidebarLabel('tracker', 'same'); return await wings.setSidebarLabel('tracker', 'same');");
    assert_eq!(rig.env.seen().len(), before + 1);
}

#[test]
fn focus_pane_needs_panes_granted_and_a_pane_that_is_there() {
    let rig = Rig::new(json!({ "permissions": { "panes": [] } }), panes());
    assert_eq!(
        rig.allowed("return await wings.focusPane('2');"),
        json!(null)
    );
    assert_eq!(rig.env.seen(), [Seen::FocusPane("2".into())]);
    assert_eq!(rig.refused("await wings.focusPane('9');"), "no pane 9");
    let without = Rig::new(json!({}), panes());
    assert_eq!(
        without.refused("await wings.focusPane('1');"),
        "probe may not focus panes"
    );
}
