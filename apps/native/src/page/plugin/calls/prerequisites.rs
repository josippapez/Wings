use crate::page::plugin::{
    Project,
    testing::{Frame, PRELUDE, Rig, Seen},
};
use serde_json::json;
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[test]
fn projects_are_the_published_workspace_list_and_changes_reach_existing_frames() {
    let rig = Rig::new(json!({}), vec![]);
    let project = Project {
        id: "/repo".into(),
        path: "/repo".into(),
        name: "Repo".into(),
        parent_id: None,
    };
    rig.host.publish_projects(vec![project.clone()]);
    assert_eq!(
        rig.allowed("return await wings.projects();"),
        json!([{ "id": "/repo", "path": "/repo", "name": "Repo" }])
    );
    let frame = Frame::start(
        &rig.host,
        &format!(
            "{PRELUDE} const lists = []; wings.onProjects(list => {{ lists.push(list); show(lists); }});"
        ),
    );
    frame.report("initial projects", |r| {
        r.as_array().is_some_and(|a| a.len() == 1)
    });
    rig.host.publish_projects(vec![project]);
    rig.host.publish_projects(vec![]);
    let got = frame.report("changed projects", |r| {
        r.as_array().is_some_and(|a| a.len() == 2)
    });
    assert_eq!(got[1], json!([]));
}

#[test]
fn focus_is_current_on_start_and_only_changes_are_delivered() {
    let rig = Rig::new(json!({}), vec![]);
    rig.host.publish_focus(false);
    let frame = Frame::start(
        &rig.host,
        &format!(
            "{PRELUDE} const states = []; wings.onFocus(value => {{ states.push([value.focused, wings.focused]); show(states); }});"
        ),
    );
    assert_eq!(
        frame.report("initial focus", |r| r
            .as_array()
            .is_some_and(|a| a.len() == 1)),
        json!([[false, false]])
    );
    rig.host.publish_focus(false);
    rig.host.publish_focus(true);
    assert_eq!(
        frame.report("focus change", |r| r
            .as_array()
            .is_some_and(|a| a.len() == 2)),
        json!([[false, false], [true, true]])
    );
}

#[test]
fn git_events_need_a_git_exec_entry_and_unsubscribe_works() {
    for (entries, expected) in [
        (json!(["git log []"]), 1),
        (json!(["git[]"]), 1),
        (json!(["gitx", "gh api"]), 0),
    ] {
        let rig = Rig::new(json!({ "permissions": { "exec": entries } }), vec![]);
        let frame = Frame::start(
            &rig.host,
            &format!(
                "{PRELUDE} let count = 0, barriers = 0; const off = wings.onGitChange(value => {{ count++; show({{count, value}}); }}); wings.onBroadcast(() => {{ off(); show({{count, barriers: ++barriers}}); }}); show({{ready:true}});"
            ),
        );
        frame.report("listener ready", |r| r["ready"] == true);
        rig.host.publish_git(&["/repo".into()]);
        let barrier = Frame::start(&rig.host, "wings.broadcast({});");
        let result = frame.report("git event barrier", |r| r["barriers"] == 1);
        assert_eq!(result["count"], expected);
        rig.host.publish_git(&["/repo".into()]);
        let second = Frame::start(&rig.host, "wings.broadcast({});");
        let result = frame.report("unsubscribed git event barrier", |r| r["barriers"] == 2);
        assert_eq!(result["count"], expected);
        drop(second);
        drop(barrier);
    }
}

#[test]
fn disks_and_search_require_permission_and_search_validates_names_and_paths() {
    let denied = Rig::new(json!({}), vec![]);
    assert_eq!(
        denied.refused("return await wings.disks();"),
        "probe may not read disk usage"
    );
    assert_eq!(
        denied.refused("return await wings.diskFind('/', { names: ['.git'] });"),
        "probe may not read disk usage"
    );
    let rig = Rig::new(json!({ "permissions": { "diskUsage": true } }), vec![]);
    assert!(rig.allowed("return await wings.disks();").is_array());
    for body in [
        "return await wings.diskFind('', { names: ['.git'] });",
        "return await wings.diskFind('/', { names: [] });",
        "return await wings.diskFind('/', { names: ['a/b'] });",
        "return await wings.diskFind('/', { names: ['.git'], skip: [5] });",
    ] {
        assert!(rig.refused(body).contains("diskFind"));
    }
    assert_eq!(rig.refused("const stop = new AbortController(); stop.abort(); return await wings.diskFind('/', { names: ['.git'], signal: stop.signal });"), "The scan was stopped");
    let root = rig.root.config("probe").root.join("search");
    fs::create_dir_all(root.join("repo/.git")).unwrap();
    fs::create_dir_all(root.join("skip/repo/.git")).unwrap();
    let result = rig.allowed(&format!(
        "return await wings.diskFind({}, {{ names: ['.git'], skip: ['skip'] }});",
        json!(root)
    ));
    assert_eq!(
        result["paths"],
        json!([root.join("repo/.git").canonicalize().unwrap()])
    );
    assert!(result["entries"].as_u64().unwrap() >= 3);
    assert!(rig.host.shared.scans.lock().unwrap().is_empty());
}

#[test]
fn closing_a_frame_stops_only_its_searches() {
    let rig = Rig::new(json!({}), vec![]);
    let own = Arc::new(AtomicBool::new(false));
    let other = Arc::new(AtomicBool::new(false));
    rig.host.shared.scans.lock().unwrap().extend([
        ("42:scan".into(), own.clone()),
        ("43:scan".into(), other.clone()),
    ]);
    drop(crate::page::plugin::Membership {
        id: 42,
        shared: rig.host.shared.clone(),
    });
    assert!(own.load(Ordering::Relaxed));
    assert!(!other.load(Ordering::Relaxed));
    drop(rig.host);
    assert!(other.load(Ordering::Relaxed));
}

#[test]
fn opening_a_pane_uses_shared_permissions_and_resolves_its_project() {
    let rig = Rig::new(json!({ "permissions": { "panes": ["echo"] } }), vec![]);
    let path = rig.root.config("probe").root.canonicalize().unwrap();
    *rig.env.projects.lock().unwrap() = vec![path.clone()];
    assert_eq!(rig.allowed("return await wings.openPane({ command: 'echo', args: ['hello there'], placement: 'right' });"), json!({ "paneId": "3" }));
    let Seen::OpenPane(plan) = &rig.env.seen()[0] else {
        panic!("no pane plan")
    };
    assert_eq!(plan.cwd, path);
    assert_eq!(plan.input.as_deref(), Some("echo 'hello there'\r"));
    assert_eq!(plan.placement, "right");
    for body in [
        "await wings.openPane({ command: 'sh' });",
        "await wings.openPane({ args: [5] });",
        "await wings.openPane({ placement: 'outside' });",
        "await wings.openPane({ cwd: '/' });",
        "await wings.openPane({ cwd: 5 });",
    ] {
        assert!(!rig.refused(body).is_empty());
    }
    assert_eq!(
        Rig::new(json!({}), vec![]).refused("await wings.openPane();"),
        "probe may not open panes"
    );
}

#[test]
fn opening_a_view_checks_the_manifest_placement_and_arguments() {
    let rig = Rig::new(
        json!({ "contributes": { "views": [{ "id": "history", "title": "History", "page": "history.html", "opens": ["sidebar", "tab"] }] } }),
        vec![],
    );
    assert_eq!(rig.allowed("await wings.openView('history', { as: 'tab', args: { cwd: '/repo' }, newTab: true }); return 1;"), 1);
    let Seen::OpenView(plan) = &rig.env.seen()[0] else {
        panic!("no view plan")
    };
    assert_eq!(plan.place, "tab");
    assert_eq!(plan.args, Some(json!({ "cwd": "/repo" })));
    assert!(plan.new_tab);
    for body in [
        "await wings.openView('missing');",
        "await wings.openView('history', { as: 'window' });",
        "await wings.openView('history', { args: {} });",
        "await wings.openView('history', { newTab: true });",
        "await wings.openView('history', { as: 'tab', args: 'x'.repeat(17000) });",
    ] {
        assert!(!rig.refused(body).is_empty());
    }
}
