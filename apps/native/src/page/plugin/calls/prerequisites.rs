use crate::page::{
    plugin::{
        FrameState, Project, StatePlace,
        testing::{Caller, Frame, Rig, Seen, barrier, hears, open_pane, show},
    },
    runtime::fake::{self, Cx, Guest},
};
use serde_json::{Value, json};
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
        parent_id: None, hidden_sections: Vec::new(),
    };
    rig.host.publish_projects(vec![project.clone()]);
    assert_eq!(
        rig.allows("projects", json!({})),
        json!([{ "id": "/repo", "path": "/repo", "name": "Repo" }])
    );
    let frame = Frame::start(&rig.host, &hears("projects"));
    frame.report("initial projects", |r| {
        r["heard"].as_array().is_some_and(|a| a.len() == 1)
    });
    rig.host.publish_projects(vec![project]);
    rig.host.publish_projects(vec![]);
    let got = frame.report("changed projects", |r| {
        r["heard"].as_array().is_some_and(|a| a.len() == 2)
    });
    assert_eq!(got["heard"][1], json!([]));
}

/// A page that shows whether the frame was focused as it started, and each focus it hears.
struct Focus {
    at_start: bool,
    heard: Vec<Value>,
}

impl Guest for Focus {
    fn start(&mut self, cx: &mut Cx<'_>) {
        self.at_start = cx.link().focused();
    }

    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, text: &str) {
        if kind == "focus" {
            self.heard.push(serde_json::from_str::<Value>(text).unwrap()["focused"].clone());
            show(cx, json!({ "atStart": self.at_start, "heard": self.heard }));
        }
    }
}

#[test]
fn focus_is_current_on_start_and_only_changes_are_delivered() {
    let rig = Rig::new(json!({}), vec![]);
    rig.host.publish_focus(false);
    let source = fake::source("focus", || Box::new(Focus { at_start: true, heard: Vec::new() }));
    let frame = Frame::start(&rig.host, &source);
    assert_eq!(
        frame.report("initial focus", |r| r["heard"].as_array().is_some_and(|a| a.len() == 1)),
        json!({ "atStart": false, "heard": [false] })
    );
    rig.host.publish_focus(false);
    rig.host.publish_focus(true);
    assert_eq!(
        frame.report("focus change", |r| r["heard"].as_array().is_some_and(|a| a.len() == 2)),
        json!({ "atStart": false, "heard": [false, true] })
    );
}

/// A page that counts the git changes it hears until a broadcast comes, which is when it stops listening.
struct Git {
    count: u32,
    barriers: u32,
    off: bool,
}

impl Guest for Git {
    fn start(&mut self, cx: &mut Cx<'_>) {
        show(cx, json!({ "ready": true }));
    }

    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, _text: &str) {
        match kind {
            "git" if !self.off => {
                self.count += 1;
                show(cx, json!({ "count": self.count }));
            }
            "broadcast" => {
                self.off = true;
                self.barriers += 1;
                show(cx, json!({ "count": self.count, "barriers": self.barriers }));
            }
            _ => {}
        }
    }
}

#[test]
fn git_events_need_a_git_exec_entry_and_unsubscribe_works() {
    for (entries, expected) in [
        (json!(["git log []"]), 1),
        (json!(["git[]"]), 1),
        (json!(["gitx", "gh api"]), 0),
    ] {
        let rig = Rig::new(json!({ "permissions": { "exec": entries } }), vec![]);
        let source = fake::source("git", || Box::new(Git { count: 0, barriers: 0, off: false }));
        let frame = Frame::start(&rig.host, &source);
        frame.report("listener ready", |r| r["ready"] == true);
        rig.host.publish_git(&["/repo".into()]);
        let barrier_one = Frame::start(&rig.host, &barrier());
        let result = frame.report("git event barrier", |r| r["barriers"] == 1);
        assert_eq!(result["count"], expected);
        rig.host.publish_git(&["/repo".into()]);
        let second = Frame::start(&rig.host, &barrier());
        let result = frame.report("unsubscribed git event barrier", |r| r["barriers"] == 2);
        assert_eq!(result["count"], expected);
        drop(second);
        drop(barrier_one);
    }
}

#[test]
fn disks_and_search_require_permission_and_search_validates_names_and_paths() {
    let find = |path: &str, options: Value| {
        let mut params = json!({ "path": path, "token": "1", "stream": false });
        params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        params
    };
    let denied = Rig::new(json!({}), vec![]);
    assert_eq!(
        denied.refuses("disks", json!({})),
        "probe may not read disk usage"
    );
    assert_eq!(
        denied.refuses("diskFind", find("/", json!({ "names": [".git"] }))),
        "probe may not read disk usage"
    );
    let rig = Rig::new(json!({ "permissions": { "diskUsage": true } }), vec![]);
    assert!(rig.allows("disks", json!({})).is_array());
    for (path, options) in [
        ("", json!({ "names": [".git"] })),
        ("/", json!({ "names": [] })),
        ("/", json!({ "names": ["a/b"] })),
        ("/", json!({ "names": [".git"], "skip": [5] })),
    ] {
        assert!(rig.refuses("diskFind", find(path, options)).contains("diskFind"));
    }
    let root = rig.root.config("probe").root.join("search");
    fs::create_dir_all(root.join("repo/.git")).unwrap();
    fs::create_dir_all(root.join("skip/repo/.git")).unwrap();
    let result = rig.allows(
        "diskFind",
        find(root.to_str().unwrap(), json!({ "names": [".git"], "skip": ["skip"] })),
    );
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
    assert_eq!(
        rig.allows(
            "openPane",
            open_pane(json!({ "command": "echo", "args": ["hello there"], "placement": "right" }))
        ),
        json!({ "paneId": "3" })
    );
    let Seen::OpenPane(plan) = &rig.env.seen()[0] else {
        panic!("no pane plan")
    };
    assert_eq!(plan.cwd, path);
    assert_eq!(plan.input.as_deref(), Some("echo 'hello there'\r"));
    assert_eq!(plan.placement, "right");
    for extra in [
        json!({ "command": "sh" }),
        json!({ "args": [5] }),
        json!({ "placement": "outside" }),
        json!({ "cwd": "/" }),
        json!({ "cwd": 5 }),
    ] {
        assert!(!rig.refuses("openPane", open_pane(extra)).is_empty());
    }
    assert_eq!(
        Rig::new(json!({}), vec![]).refuses("openPane", open_pane(json!({}))),
        "probe may not open panes"
    );
}

fn tab() -> FrameState {
    FrameState::new(StatePlace::Tab, None, |_| Ok(()))
}

#[test]
fn a_pane_takes_the_place_of_the_tab_of_the_page_that_asks_and_only_a_page_shown_as_a_tab_may_ask() {
    let rig = Rig::new(json!({ "permissions": { "panes": ["echo"] } }), vec![]);
    let path = rig.root.config("probe").root.canonicalize().unwrap();
    *rig.env.projects.lock().unwrap() = vec![path];
    let state = tab();
    let page = Caller::in_tab(&rig.host, state.clone());
    let replace = open_pane(json!({ "command": "echo", "placement": "replace" }));
    assert_eq!(page.call("openPane", replace.clone()), Ok(json!({ "paneId": "3" })));
    let Seen::OpenPane(plan) = &rig.env.seen()[0] else {
        panic!("no pane plan")
    };
    assert_eq!(plan.placement, "tab");
    assert_eq!(plan.replace, Some(("probe".into(), state.frame().unwrap())));
    assert_eq!(
        rig.refuses("openPane", replace),
        "openPane placement \"replace\" works only from a page shown as a tab"
    );
}

#[test]
fn past_sessions_open_to_resume_in_the_tab_of_a_page_whose_plugin_resumes_sessions() {
    let views = json!([{ "id": "chat", "title": "Chat", "page": "chat.html", "opens": ["tab"], "resumes": ["claude"] }]);
    let rig = Rig::new(json!({ "contributes": { "views": views } }), vec![]);
    let page = Caller::in_tab(&rig.host, tab());
    let here = json!({ "resumeIn": "this tab" });
    // The answer waits for the sheet: the session you resumed in the page.
    assert_eq!(page.call("openHistory", here.clone()), Ok(json!({ "resume": "s1", "cwd": "/proj" })));
    assert_eq!(rig.env.seen(), [Seen::OpenHistory(vec!["claude".into()])]);
    assert_eq!(page.call("openHistory", json!({})), Err("openHistory resumeIn is \"this tab\"".into()));
    assert_eq!(rig.refuses("openHistory", here.clone()), "openHistory works only from a page shown as a tab");
    let plain = Rig::new(json!({}), vec![]);
    let page = Caller::in_tab(&plain.host, tab());
    assert_eq!(page.call("openHistory", here), Err("probe has no view that resumes sessions".into()));
}

#[test]
fn opening_a_view_checks_the_manifest_placement_and_arguments() {
    let rig = Rig::new(
        json!({ "contributes": { "views": [{ "id": "history", "title": "History", "page": "history.html", "opens": ["sidebar", "tab"] }] } }),
        vec![],
    );
    // What `wings.openView(id, options)` sends: `newTab` is always there, and the other options when they were given.
    let open = |id: &str, options: Value| {
        let mut params = json!({ "id": id, "newTab": false });
        params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        params
    };
    rig.allows(
        "openView",
        open("history", json!({ "as": "tab", "args": { "cwd": "/repo" }, "newTab": true })),
    );
    let Seen::OpenView(plan) = &rig.env.seen()[0] else {
        panic!("no view plan")
    };
    assert_eq!(plan.place, "tab");
    assert_eq!(plan.args, Some(json!({ "cwd": "/repo" })));
    assert!(plan.new_tab);
    assert_eq!(plan.project, None);
    for (id, options) in [
        ("missing", json!({})),
        ("history", json!({ "as": "window" })),
        ("history", json!({ "args": {} })),
        ("history", json!({ "newTab": true })),
        ("history", json!({ "as": "tab", "args": "x".repeat(17000) })),
    ] {
        assert!(!rig.refuses("openView", open(id, options)).is_empty());
    }
}

#[test]
fn a_tab_opens_in_the_project_its_id_names_and_in_no_other() {
    let rig = Rig::new(
        json!({ "contributes": { "views": [{ "id": "history", "title": "History", "page": "history.html", "opens": ["sidebar", "tab"] }] } }),
        vec![],
    );
    rig.host.publish_projects(vec![Project { id: "other".into(), path: "/elsewhere".into(), name: "Other".into(), parent_id: None, hidden_sections: Vec::new() }]);
    let tab = |project: Value| json!({ "id": "history", "as": "tab", "newTab": true, "projectId": project });
    rig.allows("openView", tab(json!("other")));
    let Seen::OpenView(plan) = &rig.env.seen()[0] else {
        panic!("no view plan")
    };
    assert_eq!(plan.project, Some("/elsewhere".into()));
    // An id Wings doesn't know, and a place that has no project, open nothing.
    assert_eq!(rig.refuses("openView", tab(json!("/etc"))), "no project /etc");
    assert!(rig.refuses("openView", json!({ "id": "history", "as": "sidebar", "projectId": "other" })).contains("projectId"));
    assert!(rig.refuses("openView", json!({ "id": "history", "as": "tab", "projectId": "other" })).contains("projectId"));
    assert_eq!(rig.env.seen().len(), 1);
}

#[test]
fn a_background_pane_is_a_tab_that_is_not_shown_and_nothing_else() {
    let rig = Rig::new(json!({ "permissions": { "panes": [] } }), vec![]);
    let path = rig.root.config("probe").root.canonicalize().unwrap();
    *rig.env.projects.lock().unwrap() = vec![path];
    rig.allows("openPane", open_pane(json!({ "background": true })));
    rig.allows("openPane", open_pane(json!({ "background": false })));
    let placements: Vec<String> = rig.env.seen().into_iter().filter_map(|seen| if let Seen::OpenPane(plan) = seen { Some(plan.placement) } else { None }).collect();
    assert_eq!(placements, ["background", "tab"]);
    for wrong in [json!({ "background": "yes" }), json!({ "background": true, "placement": "right" })] {
        assert!(rig.refuses("openPane", open_pane(wrong)).contains("background"));
    }
    assert_eq!(rig.env.seen().len(), 2);
}
