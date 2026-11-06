//! Claude Code's calls of a plugin's main, as the socket hands them on: its MCP tools and its hooks (`onHook`), each with
//! the pane, project and session it came from.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::page::{
    plugin::{
        Access, ClaudeCaller, PluginHost,
        testing::{Rig, WAIT, plugin},
    },
    runtime::Reply,
};

/// A running plugin `tasks` with the tool `where` and the hooks `SessionStart` and `PreToolUse`.
fn tasks(rig: &Rig) -> Arc<PluginHost> {
    let tasks = plugin(json!({
        "id": "tasks",
        "contributes": {
            "mcpTools": [{ "name": "where", "description": "where", "inputSchema": { "type": "object" } }],
            "hooks": ["SessionStart", "PreToolUse", "UserPromptSubmit"],
        },
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
        "wings.onTool('where', (input, c) => `${c.paneId} ${c.project?.name} ${c.session?.id}`);
         wings.onHook('SessionStart', (input, c) => `hello ${input.source} from ${c.paneId} in ${c.project?.name} as ${c.session?.id}`);
         wings.onHook('PreToolUse', async (input) => ({ deny: input.tool_input.command.includes('push') ? 'no pushing' : '', context: '' }));
         wings.onHook('UserPromptSubmit', () => { throw new Error('broken'); });",
        |_| {},
    )
    .unwrap();
    rig.env.running.add(&tasks, true, &host);
    host
}

fn caller() -> ClaudeCaller {
    ClaudeCaller {
        pane_id: Some("3".into()),
        project: json!({ "id": "/work", "name": "Work", "path": "/work" }),
        session: json!({ "id": "s1", "name": null, "kind": "claude" }),
    }
}

/// What the plugin answered Claude, waiting for its main to start.
fn ask(rig: &Rig, tool: &str, input: Value, hook: bool) -> Value {
    let started = std::time::Instant::now();
    loop {
        let (reply, answer) = Reply::channel();
        rig.env
            .running
            .call_for_claude("tasks", tool, input.clone(), &caller(), hook, reply);
        match answer.wait(WAIT).expect("the plugin answers") {
            Ok(json) => return serde_json::from_str(&json).unwrap(),
            Err(still) if still.contains("still starting") && started.elapsed() < WAIT => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn a_hook_reaches_the_plugins_on_hook_with_who_called_and_its_answer_comes_back() {
    let rig = Rig::new(json!({}), vec![]);
    let _tasks = tasks(&rig);
    let start =
        json!({ "hook_event_name": "SessionStart", "source": "startup", "session_id": "s1" });
    assert_eq!(
        ask(&rig, "SessionStart", start, true),
        json!({ "text": r#"{"context":"hello startup from 3 in Work as s1"}"#, "isError": false })
    );
    // An empty context says nothing, and a refusal comes back as `deny`.
    let push = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": "git push" } });
    assert_eq!(
        ask(&rig, "PreToolUse", push, true)["text"],
        r#"{"deny":"no pushing"}"#
    );
    // A handler that throws says nothing rather than failing Claude's turn.
    assert_eq!(
        ask(&rig, "UserPromptSubmit", json!({}), true),
        json!({ "text": "{}", "isError": false })
    );
    assert_eq!(rig.env.running.hooked("PreToolUse"), ["tasks"]);
    assert!(rig.env.running.hooked("Journal").is_empty());
}

#[test]
fn a_tool_call_from_claude_carries_its_pane_project_and_session() {
    let rig = Rig::new(json!({}), vec![]);
    let _tasks = tasks(&rig);
    assert_eq!(
        ask(&rig, "where", json!({}), false),
        json!({ "text": "3 Work s1", "isError": false })
    );
    assert_eq!(rig.env.running.mcp_tools()[0]["name"], "tasks__where");
    // A hook the manifest doesn't declare, or a tool it doesn't, isn't run.
    let (reply, answer) = Reply::channel();
    rig.env
        .running
        .call_for_claude("tasks", "Journal", json!({}), &caller(), true, reply);
    assert_eq!(
        answer.wait(WAIT),
        Some(Err("tasks has no tool Journal".into()))
    );
    let (reply, answer) = Reply::channel();
    rig.env
        .running
        .call_for_claude("other", "where", json!({}), &caller(), false, reply);
    assert_eq!(
        answer.wait(WAIT),
        Some(Err("other is turned off in Wings".into()))
    );
}
