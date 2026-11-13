//! The main script: Claude's tools and hooks, the operations the pages ask for, and each project's section in the sidebar.
//! Every change goes through `writer::write`.

use crate::{
    ask::serve,
    claude_hooks,
    commands::resume_command,
    domain::{Actor, Op},
    file_diff::open_file_diff,
    pr_check::{listen_pr_state, watch_prs},
    sidebar::{on_section_panes, watch_order},
    tool_defs,
    writer::{load_project, write},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{host, task::spawn};

thread_local! {
    /// The panes as Wings last listed them.
    static PANES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn text(value: &Value, name: &str) -> String {
    value[name].as_str().unwrap_or_default().to_owned()
}

/// What a page asks for: an operation on a project's issues, a session to open, or a file's diff.
async fn answer(what: String, payload: Value) -> Result<(), String> {
    match what.as_str() {
        "op" => {
            let op = Op::from_value(&payload["op"])?;
            write(&text(&payload, "projectId"), &text(&payload, "projectName"), &op, &Actor::You).map(drop)
        }
        // A running session is focused, never started a second time.
        "open-session" => {
            let (id, kind) = (text(&payload["session"], "id"), payload["session"]["kind"].as_str());
            let pane = PANES.with(|panes| panes.borrow().iter().find(|p| p["session"]["sessionId"] == id.as_str()).map(|p| text(p, "paneId")));
            match pane {
                Some(pane) => host::call("focusPane", json!({ "paneId": pane })).await.map(drop),
                None => {
                    let command = resume_command(&id, kind)?;
                    host::call("openPane", json!({ "command": command, "args": [], "cwd": text(&payload, "cwd"), "placement": "tab" })).await.map(drop)
                }
            }
        }
        "open-diff" => open_file_diff(payload["commit"].as_str().map(str::to_owned), text(&payload, "path"), text(&payload, "root")).await,
        other => Err(format!("Issues doesn't do {other}.")),
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    tool_defs::register();
    claude_hooks::register();
    watch_prs();
    listen_pr_state();
    watch_order();
    host::on_panes(|list| {
        PANES.with(|panes| *panes.borrow_mut() = list.to_vec());
        on_section_panes(list);
    });
    serve(|what, payload| Box::pin(answer(what, payload)));
    // Picks up every project as soon as Wings tells us which exist, and again when that changes.
    host::on_projects(|list| {
        // A worktree Wings made has no store of its own: its issues are in its main project, which is listed too.
        for project in list.iter().filter(|p| p["parentId"].is_null()) {
            spawn(load_project(text(project, "id"), text(project, "name")));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kv::memory, storage::slot_of, writer::CLOCK};
    use wings_plugin::task::block_on;

    type Sections = std::rc::Rc<RefCell<Vec<(String, String, Value)>>>;

    /// A host that records the sections set under each project.
    fn host_with_sections() -> Sections {
        let sections: Sections = Sections::default();
        let seen = sections.clone();
        host::testing::answer(move |method, params| {
            match method {
                "setProjectSection" => seen.borrow_mut().push((params["projectId"].as_str().unwrap().to_owned(), params["sectionId"].as_str().unwrap().to_owned(), params["rows"].clone())),
                "projects" => return Ok(json!([])),
                _ => {}
            }
            Ok(Value::Null)
        });
        sections
    }

    fn ask(project: &str, name: &str, op: Value) -> Result<(), String> {
        let done = block_on(answer("op".into(), json!({ "projectId": project, "projectName": name, "op": op })));
        block_on(async {});
        done
    }

    fn last(sections: &Sections, project: &str) -> Value {
        sections.borrow().iter().rev().find(|(id, _, _)| id == project).map(|(_, _, rows)| rows.clone()).unwrap()
    }

    fn keys(rows: &Value) -> Vec<String> {
        rows.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn a_pages_operation_is_answered_once_it_is_saved_a_change_of_nothing_saves_nothing_and_an_error_reaches_the_page() {
        memory::reset(&[]);
        host_with_sections();
        ask("proj1", "web", json!({ "kind": "create", "title": "Do it" })).unwrap();
        ask("proj1", "web", json!({ "kind": "update", "key": "WEB-1", "title": "Done it" })).unwrap();
        assert_eq!(memory::peek(&format!("issues/{}/WEB-1", slot_of("proj1"))).unwrap()["title"], "Done it");
        memory::clear_writes();
        ask("proj1", "web", json!({ "kind": "update", "key": "WEB-1", "title": "Done it" })).unwrap();
        assert!(memory::writes().is_empty(), "a change of nothing writes nothing");
        assert!(ask("proj1", "web", json!({ "kind": "update", "key": "WEB-1", "status": "later" })).unwrap_err().contains("status must be one of"));
        assert!(block_on(answer("explode".into(), json!({}))).unwrap_err().contains("doesn't do explode"));
    }

    #[test]
    fn the_projects_section_shows_its_open_issues_after_a_change_and_is_filled_when_wings_lists_the_projects() {
        memory::reset(&[]);
        let sections = host_with_sections();
        ask("proj1", "web", json!({ "kind": "create", "title": "Second" })).unwrap();
        let row = json!([{ "id": "WEB-1", "label": "WEB-1 Second", "icon": "todo", "open": { "view": "issue", "args": { "key": "WEB-1", "projectId": "proj1" } } }]);
        assert_eq!(last(&sections, "proj1"), row);
        block_on(async {
            load_project("proj1".into(), "web".into()).await;
            load_project("proj2".into(), "api".into()).await;
        });
        assert_eq!(last(&sections, "proj1"), row);
        assert_eq!(last(&sections, "proj2"), Value::Null, "a project with no issues has no section");
    }

    #[test]
    fn the_section_lists_children_after_their_epic_or_feature_and_names_it_as_their_parent_unless_it_isnt_open() {
        memory::reset(&[]);
        let sections = host_with_sections();
        let docs = |op: Value| ask("proj3", "docs", op).unwrap();
        for issue in [json!({ "title": "Plain" }), json!({ "title": "Epic", "type": "epic" }), json!({ "title": "Feature", "type": "feature", "parent": "DOC-2" }), json!({ "title": "Started", "status": "in_progress", "parent": "DOC-2" }), json!({ "title": "Lone", "status": "in_progress" }), json!({ "title": "Deep", "parent": "DOC-3" })] {
            let mut op = json!({ "kind": "create" });
            for (name, value) in issue.as_object().unwrap() {
                op[name] = value.clone();
            }
            docs(op);
        }
        let parents = |rows: &Value| rows.as_array().unwrap().iter().filter(|r| r.get("parent").is_some()).map(|r| (r["id"].as_str().unwrap().to_owned(), r["parent"].as_str().unwrap().to_owned())).collect::<Vec<_>>();
        // Open issues run in progress first, then by key (4, 5, 1, 2, 3, 6); each one's children follow it, and theirs follow them.
        let rows = last(&sections, "proj3");
        assert_eq!(keys(&rows), ["DOC-5", "DOC-1", "DOC-2", "DOC-4", "DOC-3", "DOC-6"]);
        assert_eq!(parents(&rows), [("DOC-4".to_owned(), "DOC-2".to_owned()), ("DOC-3".to_owned(), "DOC-2".to_owned()), ("DOC-6".to_owned(), "DOC-3".to_owned())]);
        // With the epic done, its children are top-level rows again, with no parent to name.
        docs(json!({ "kind": "update", "key": "DOC-2", "status": "done" }));
        let rows = last(&sections, "proj3");
        assert_eq!(keys(&rows), ["DOC-4", "DOC-5", "DOC-1", "DOC-3", "DOC-6"]);
        assert_eq!(parents(&rows), [("DOC-6".to_owned(), "DOC-3".to_owned())]);
    }

    #[test]
    fn the_section_puts_the_issue_an_agent_works_on_now_first_sorts_again_when_its_pane_goes_and_follows_the_sidebar_order() {
        memory::reset(&[]);
        let sections = host_with_sections();
        let app = json!({ "id": "proj5", "name": "app", "path": "/work/app" });
        let as_session = |session: Value, name: &str, input: Value| block_on(tool_defs::run(name.to_owned(), input, tool_defs::context_of(app.clone(), session))).unwrap();
        as_session(Value::Null, "create", json!({ "title": "First" }));
        as_session(json!({ "id": "s-9", "name": "agent nine" }), "create", json!({ "title": "Second" }));
        let pane = json!({ "paneId": "p5", "projectId": "proj5", "cwd": "/work/app", "command": "claude", "project": "app", "session": { "sessionId": "s-9", "name": "agent nine", "state": "working", "startedAt": 1 }, "focused": false });
        on_section_panes(&[pane.clone()]);
        block_on(async {});
        assert_eq!(keys(&last(&sections, "proj5")), ["APP-2", "APP-1"]);
        on_section_panes(&[json!({ "paneId": "p5", "session": null })]);
        block_on(async {});
        assert_eq!(keys(&last(&sections, "proj5")), ["APP-1", "APP-2"]);
        CLOCK.set(5.0);
        as_session(Value::Null, "update", json!({ "key": "APP-1", "title": "First, renamed" }));
        assert_eq!(keys(&last(&sections, "proj5")), ["APP-1", "APP-2"], "by status, the older key stays first");
        crate::settings::set_section_order("date").unwrap();
        block_on(async {});
        // What the sidebar header's menu says is the same setting.
        CLOCK.set(9.0);
        as_session(Value::Null, "update", json!({ "key": "APP-2", "title": "Second, renamed" }));
        assert_eq!(keys(&last(&sections, "proj5")), ["APP-2", "APP-1"], "by latest change, the newest edit is first");
    }
}
