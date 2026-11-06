//! The calls Schedules makes that the plugins ported before it didn't: another plugin's tools, closing a view, the
//! project a page is about.

use std::sync::Arc;

use serde_json::json;

use crate::page::plugin::{
    Access, PluginHost, Project,
    testing::{Rig, Seen, plugin},
};

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
    host.start_main(
        "wings.onTool('list', (input, context) => `got ${JSON.stringify(input)} in ${context.project?.name}`);
         wings.onTool('boom', () => { throw new Error('nope'); });
         wings.onTool('quiet', () => {});",
        |_| {},
    )
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
        parent_id: None,
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
        rig.allowed(
            "return await wings.callTool('tasks__list', { query: 'x' }, { project: '/work' });"
        ),
        json!({ "text": "got {\"query\":\"x\"} in Work", "isError": false })
    );
    // Without a project it is the one on screen.
    assert_eq!(
        rig.allowed("return await wings.callTool('tasks__list');"),
        json!({ "text": "got {} in Work", "isError": false })
    );
    assert_eq!(
        rig.allowed("return await wings.callTool('tasks__boom');"),
        json!({ "text": "nope", "isError": true })
    );
    assert_eq!(
        rig.allowed("return await wings.callTool('tasks__quiet');"),
        json!({ "text": "Done", "isError": false })
    );
    assert_eq!(
        rig.refused("return await wings.callTool('tasks__missing');"),
        "tasks has no tool missing"
    );
    assert_eq!(
        rig.refused("return await wings.callTool('tasks__list', {}, { project: '/elsewhere' });"),
        "no project /elsewhere"
    );
}

#[test]
fn a_tool_call_is_refused_unless_both_plugins_are_wings_own_the_manifest_names_the_tool_and_the_other_runs()
 {
    let permitted = json!({ "callTools": ["tasks list"] });
    assert_eq!(
        caller(permitted.clone(), false).refused("return await wings.callTool('tasks__list');"),
        "Only Wings' own plugins can call each other's tools"
    );
    let rig = caller(json!({ "callTools": ["tasks quiet"] }), true);
    assert_eq!(
        rig.refused("return await wings.callTool('tasks__list');"),
        "probe may not call tasks's list"
    );
    for name in ["tasks", "__list", "undefined"] {
        assert_eq!(
            rig.refused(&format!("return await wings.callTool('{name}');")),
            r#"callTool takes a name like "tasks__list""#,
            "{name}"
        );
    }
    let rig = caller(permitted, true);
    assert_eq!(
        rig.refused("return await wings.callTool('tasks__list');"),
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
        rig.refused("return await wings.callTool('tasks__list');"),
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
        rig.refused("return await wings.closeView('nope');"),
        "probe has no view nope"
    );
    assert_eq!(
        rig.allowed("return await wings.closeView('approve');"),
        json!(null)
    );
    assert_eq!(
        rig.env.seen().last(),
        Some(&Seen::CloseView("probe".into(), "approve".into()))
    );

    let rig = caller(json!({}), false);
    assert_eq!(
        rig.allowed("return await wings.project();"),
        json!({ "id": "/work", "path": "/work", "name": "Work" })
    );
    *rig.env.projects.lock().unwrap() = Vec::new();
    assert_eq!(rig.allowed("return await wings.project();"), json!(null));
}

#[test]
fn worktree_calls_are_refused_without_the_permission() {
    // No `worktrees` permission: the desktop's words for each call.
    let rig = caller(json!({}), true);
    for (call, refused) in [
        (
            "createWorktree({ branch: 'x' })",
            "probe may not create worktrees",
        ),
        (
            "worktreeSetup('/work')",
            "probe may not read worktree setup",
        ),
        (
            "setWorktreeSetup('/work', { include: [] })",
            "probe may not change worktree setup",
        ),
    ] {
        assert_eq!(
            rig.refused(&format!("return await wings.{call};")),
            refused,
            "{call}"
        );
    }
}
