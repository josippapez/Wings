//! The calls Schedules makes that the plugins ported before it didn't: another plugin's tools, closing a view, the
//! project a page is about.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::page::{
    plugin::{
        Access, PluginHost, Project,
        testing::{Rig, Seen, plugin},
    },
    runtime::fake::{self, Cx, Guest},
};

/// The main of `tasks`: answers its tools as `onTool` handlers do, with a `toolDone` call.
struct Tasks;

impl Guest for Tasks {
    fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, token: u32, text: &str) {
        if kind != "tool" {
            return;
        }
        let call: Value = serde_json::from_str(text).unwrap();
        let (text, is_error) = match call["name"].as_str() {
            Some("list") => {
                let project = call["project"]["name"].as_str().unwrap_or("undefined");
                (format!("got {} in {project}", call["arguments"]), false)
            }
            Some("boom") => ("nope".to_owned(), true),
            _ => ("Done".to_owned(), false),
        };
        cx.call("toolDone", json!({ "token": token, "text": text, "isError": is_error }));
    }
}

/// What `wings.callTool(name, args, { project })` sends the host.
fn tool(name: &str, arguments: Value, project: Value) -> Value {
    json!({ "name": name, "arguments": arguments, "project": project })
}

/// A running plugin `tasks` with the tools `list`, `boom` and `quiet`, whose main answers them.
fn tasks(rig: &Rig) -> Arc<PluginHost> {
    let tool = |name: &str| json!({ "name": name, "description": name, "inputSchema": { "type": "object" } });
    let tasks = plugin(json!({
        "id": "tasks",
        "contributes": { "mcpTools": [tool("list"), tool("boom"), tool("quiet")] },
    }));
    let host = Arc::new(PluginHost::with_access(
        rig.root.config("tasks"),
        Access {
            plugin: tasks.clone(),
            builtin: true,
            env: rig.env.clone(),
        },
    ));
    host.start_main(&fake::source("main", || Box::new(Tasks)), |_| {})
    .unwrap();
    rig.env.running.add(&tasks, true, &host);
    host
}

fn caller(permissions: serde_json::Value, builtin: bool) -> Rig {
    let rig = Rig::with(
        plugin(json!({ "permissions": permissions })),
        builtin,
        vec![],
    );
    *rig.env.projects.lock().unwrap() = vec!["/work".into()];
    rig.host.publish_projects(vec![Project {
        id: "/work".into(),
        path: "/work".into(),
        name: "Work".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    rig
}

#[test]
fn a_tool_call_runs_the_other_plugins_handler_with_the_project_and_answers_its_text() {
    let rig = caller(
        json!({ "callTools": ["tasks list", "tasks boom", "tasks quiet", "tasks missing"] }),
        true,
    );
    let _tasks = tasks(&rig);
    assert_eq!(
        rig.allows("callTool", tool("tasks__list", json!({ "query": "x" }), json!("/work"))),
        json!({ "text": "got {\"query\":\"x\"} in Work", "isError": false })
    );
    // Without a project it is the one on screen.
    assert_eq!(
        rig.allows("callTool", tool("tasks__list", json!({}), json!(null))),
        json!({ "text": "got {} in Work", "isError": false })
    );
    assert_eq!(
        rig.allows("callTool", tool("tasks__boom", json!({}), json!(null))),
        json!({ "text": "nope", "isError": true })
    );
    assert_eq!(
        rig.allows("callTool", tool("tasks__quiet", json!({}), json!(null))),
        json!({ "text": "Done", "isError": false })
    );
    assert_eq!(
        rig.refuses("callTool", tool("tasks__missing", json!({}), json!(null))),
        "tasks has no tool missing"
    );
    assert_eq!(
        rig.refuses("callTool", tool("tasks__list", json!({}), json!("/elsewhere"))),
        "no project /elsewhere"
    );
}

#[test]
fn a_tool_call_is_refused_unless_both_plugins_are_wings_own_the_manifest_names_the_tool_and_the_other_runs()
 {
    let permitted = json!({ "callTools": ["tasks list"] });
    let list = || tool("tasks__list", json!({}), json!(null));
    assert_eq!(
        caller(permitted.clone(), false).refuses("callTool", list()),
        "Only Wings' own plugins can call each other's tools"
    );
    let rig = caller(json!({ "callTools": ["tasks quiet"] }), true);
    assert_eq!(
        rig.refuses("callTool", list()),
        "probe may not call tasks's list"
    );
    for name in ["tasks", "__list", "undefined"] {
        assert_eq!(
            rig.refuses("callTool", tool(name, json!({}), json!(null))),
            r#"callTool takes a name like "tasks__list""#,
            "{name}"
        );
    }
    let rig = caller(permitted, true);
    assert_eq!(
        rig.refuses("callTool", list()),
        "tasks is turned off or not installed"
    );
    // Running, but its main hasn't started.
    let tasks = plugin(json!({ "id": "tasks", "contributes": { "mcpTools": [
        { "name": "list", "description": "list", "inputSchema": { "type": "object" } }] } }));
    let host = Arc::new(PluginHost::with_access(
        rig.root.config("tasks"),
        Access {
            plugin: tasks.clone(),
            builtin: true,
            env: rig.env.clone(),
        },
    ));
    rig.env.running.add(&tasks, true, &host);
    assert_eq!(
        rig.refuses("callTool", list()),
        "tasks is still starting in Wings. Try again in a moment."
    );
}

#[test]
fn a_view_closes_only_if_the_plugin_has_it_and_the_project_is_the_one_on_screen() {
    let rig = Rig::new(
        json!({ "contributes": { "views": [
            { "id": "approve", "title": "Approve", "page": "dist/approve.html", "opens": ["modal"] }] } }),
        vec![],
    );
    assert_eq!(
        rig.refuses("closeView", json!({ "id": "nope" })),
        "probe has no view nope"
    );
    assert_eq!(
        rig.allows("closeView", json!({ "id": "approve" })),
        json!(null)
    );
    assert_eq!(
        rig.env.seen().last(),
        Some(&Seen::CloseView("probe".into(), "approve".into()))
    );

    let rig = caller(json!({}), false);
    assert_eq!(
        rig.allows("project", json!({})),
        json!({ "id": "/work", "path": "/work", "name": "Work" })
    );
    *rig.env.projects.lock().unwrap() = Vec::new();
    assert_eq!(rig.allows("project", json!({})), json!(null));
}

#[test]
fn worktree_calls_are_refused_without_the_permission() {
    // No `worktrees` permission: the desktop's words for each call.
    let rig = caller(json!({}), true);
    for (method, params, refused) in [
        (
            "createWorktree",
            json!({ "branch": "x" }),
            "probe may not create worktrees",
        ),
        (
            "worktreeSetup",
            json!({ "projectId": "/work" }),
            "probe may not read worktree setup",
        ),
        (
            "setWorktreeSetup",
            json!({ "projectId": "/work", "include": [] }),
            "probe may not change worktree setup",
        ),
    ] {
        assert_eq!(rig.refuses(method, params), refused, "{method}");
    }
}

/// A running plugin `claude-chat` with a `chat` view that opens as a tab, a `side` one that is a sidebar only and no other.
fn chat_plugin(rig: &Rig, builtin: bool) -> Arc<PluginHost> {
    let chat = plugin(json!({
        "id": "claude-chat",
        "contributes": { "views": [
            { "id": "chat", "title": "Chat", "page": "chat.wasm", "opens": ["tab"] },
            { "id": "side", "title": "Side", "page": "side.wasm", "opens": ["sidebar"] },
        ] },
    }));
    let host = Arc::new(PluginHost::with_access(
        rig.root.config("claude-chat"),
        Access { plugin: chat.clone(), builtin, env: rig.env.clone() },
    ));
    rig.env.running.add(&chat, builtin, &host);
    host
}

/// What `wings.openView` sends for another plugin's view.
fn open_other(plugin: &str, id: &str, extra: Value) -> Value {
    let mut params = json!({ "plugin": plugin, "id": id, "as": "tab", "args": { "cwd": "/work", "prompt": "hi" }, "newTab": true });
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    params
}

#[test]
fn another_plugins_view_opens_as_a_new_tab_with_its_args_only_with_the_manifest_entry() {
    let rig = caller(json!({ "openViews": ["claude-chat chat"] }), true);
    let _chat = chat_plugin(&rig, true);
    assert_eq!(rig.allows("openView", open_other("claude-chat", "chat", json!({}))), Value::Null);
    let Seen::OpenView(plan) = rig.env.seen().last().cloned().unwrap() else { panic!("no view plan") };
    assert_eq!(plan.plugin, "claude-chat");
    assert_eq!(plan.id, "chat");
    assert_eq!(plan.place, "tab");
    assert_eq!(plan.args, Some(json!({ "cwd": "/work", "prompt": "hi" })));
    assert!(plan.new_tab);
    // Without `as` it is a tab all the same, the only place it can open from here.
    let mut bare = open_other("claude-chat", "chat", json!({}));
    bare.as_object_mut().unwrap().remove("as");
    rig.allows("openView", bare);

    // A view the manifest doesn't name, and the same view without any entry, are refused.
    assert_eq!(
        rig.refuses("openView", open_other("claude-chat", "side", json!({}))),
        "probe may not open claude-chat's side"
    );
    let none = caller(json!({ "callTools": ["claude-chat"] }), true);
    let _chat = chat_plugin(&none, true);
    assert_eq!(
        none.refuses("openView", open_other("claude-chat", "chat", json!({}))),
        "probe may not open claude-chat's chat"
    );
    // Only a tab: not a sidebar, a window, or a tab placed after the caller's.
    let rig = caller(json!({ "openViews": ["claude-chat chat", "claude-chat side"] }), true);
    let _chat = chat_plugin(&rig, true);
    assert_eq!(
        rig.refuses("openView", open_other("claude-chat", "side", json!({}))),
        "claude-chat has no view side that opens as a tab"
    );
    assert!(!rig.refuses("openView", open_other("claude-chat", "chat", json!({ "as": "window" }))).is_empty());
    // Views of a plugin that isn't there or isn't on are refused even with the entry.
    assert_eq!(
        rig.refuses("openView", open_other("tasks", "chat", json!({}))),
        "Only Wings' own plugins can open each other's views"
    );
    // Nothing was opened by any refused call.
    assert!(rig.env.seen().is_empty(), "{:?}", rig.env.seen());
}

#[test]
fn another_plugins_view_is_refused_for_a_caller_or_a_target_that_is_not_wings_own_or_not_running() {
    let permitted = json!({ "openViews": ["claude-chat chat"] });
    let third_party = caller(permitted.clone(), false);
    let _chat = chat_plugin(&third_party, true);
    assert_eq!(
        third_party.refuses("openView", open_other("claude-chat", "chat", json!({}))),
        "Only Wings' own plugins can open each other's views"
    );
    let not_running = caller(permitted.clone(), true);
    assert_eq!(
        not_running.refuses("openView", open_other("claude-chat", "chat", json!({}))),
        "Only Wings' own plugins can open each other's views"
    );
    let outside = caller(permitted, true);
    let _chat = chat_plugin(&outside, false);
    assert_eq!(
        outside.refuses("openView", open_other("claude-chat", "chat", json!({}))),
        "Only Wings' own plugins can open each other's views"
    );
    for rig in [&third_party, &not_running, &outside] {
        assert!(rig.env.seen().is_empty());
    }
}
