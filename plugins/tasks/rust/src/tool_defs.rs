//! The five tools Claude calls. Their descriptions and input schemas are the manifest's `mcpTools`, which this reads, so what
//! Claude is told and what a call is checked against are one text. Each call is checked against its schema before a handler
//! runs, so a handler only sees what the schema allows.

use crate::{
    actor::caller_of,
    domain::{IssueType, Op, Relations, Status, Update, normalize_key},
    format::{format_issue, format_list},
    home::{home_in, pick_project},
    storage::read_store,
    writer::write,
};
use serde_json::Value;
use wings_plugin::{host, tools};

const MANIFEST: &str = include_str!("../../wings-plugin.json");

/// What `update` can change, in the order its schema lists it.
const CHANGES: [&str; 13] = ["title", "description", "status", "priority", "branch", "pr", "commit", "type", "parent", "related_add", "related_remove", "blocked_by_add", "blocked_by_remove"];

/// `pattern` is left out of a schema: the tools module can't check one, and says so by refusing the tool. The commit's
/// shape is checked by the operation, which says what is wrong.
fn without_pattern(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.remove("pattern");
            map.values_mut().for_each(without_pattern);
        }
        Value::Array(list) => list.iter_mut().for_each(without_pattern),
        _ => {}
    }
}

/// The name and input schema of each tool in the manifest.
pub fn schemas() -> Vec<(String, Value)> {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    manifest["contributes"]["mcpTools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tool| {
            let mut schema = tool["inputSchema"].clone();
            without_pattern(&mut schema);
            (tool["name"].as_str().unwrap_or_default().to_owned(), schema)
        })
        .collect()
}

pub fn register() {
    for (name, schema) in schemas() {
        let tool = name.clone();
        tools::register(&name, schema, move |input, context| {
            let tool = tool.clone();
            async move { run(tool, input, context).await.map(Value::String) }
        });
    }
}

fn text(input: &Value, name: &str) -> Option<String> {
    input[name].as_str().map(str::to_owned)
}

fn keys(input: &Value, name: &str) -> Option<Vec<String>> {
    input[name].as_array().map(|items| items.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect())
}

/// The relation fields of a call, as the operations name them.
fn relations_of(input: &Value) -> Relations {
    Relations {
        kind: text(input, "type"),
        parent: text(input, "parent"),
        related_add: keys(input, "related_add"),
        related_remove: keys(input, "related_remove"),
        blocked_by_add: keys(input, "blocked_by_add"),
        blocked_by_remove: keys(input, "blocked_by_remove"),
    }
}

/// The project a read is for: the one `project` names, else the caller's. A worktree reads its main project's issues.
async fn reader_of(context: &Value, project: Option<String>) -> Result<(String, String), String> {
    let Some(project) = project else {
        let caller = caller_of(context.clone()).await?;
        return Ok((caller.project_id, caller.project_name));
    };
    let projects = host::projects().await.unwrap_or_default();
    let projects = projects.as_array().map_or(&[][..], Vec::as_slice);
    let home = home_in(pick_project(projects, &project)?, projects);
    Ok((home.id, home.name))
}

/// Applies `op` in the project of the terminal the agent is in, as the agent session running there.
async fn edit(context: &Value, op: Op) -> Result<(String, crate::writer::Written), String> {
    let caller = caller_of(context.clone()).await?;
    let written = write(&caller.project_id, &caller.project_name, &op, &caller.actor)?;
    Ok((caller.project_name, written))
}

/// Runs the tool `name` for an input that fits its schema.
pub async fn run(name: String, input: Value, context: Value) -> Result<String, String> {
    match name.as_str() {
        "list" => {
            let (id, project_name) = reader_of(&context, text(&input, "project")).await?;
            let store = read_store(&id, &project_name)?;
            let status = text(&input, "status").and_then(|s| Status::parse(&s));
            let kind = text(&input, "type").and_then(|s| IssueType::parse(&s));
            Ok(format_list(&store.issues, status, &project_name, kind, input["query"].as_str()))
        }
        "get" => {
            let (id, project_name) = reader_of(&context, text(&input, "project")).await?;
            let store = read_store(&id, &project_name)?;
            let wanted = normalize_key(input["key"].as_str().unwrap_or_default());
            let issue = store.issues.iter().find(|i| i.key == wanted).ok_or_else(|| format!("No issue {wanted} in {project_name}. Use list to see the keys."))?;
            Ok(format_issue(issue, &store.issues))
        }
        "create" => {
            let mut create = Op::create(input["title"].as_str().unwrap_or_default());
            create.description = text(&input, "description");
            create.status = text(&input, "status");
            create.priority = text(&input, "priority");
            create.rel = relations_of(&input);
            let (project_name, written) = edit(&context, Op::Create(create)).await?;
            Ok(format!("Created {} in {project_name}. Log progress with log, and set the status with update as it moves.", written.issue.key))
        }
        "update" => {
            if CHANGES.iter().all(|field| input.get(field).is_none()) {
                let (last, rest) = CHANGES.split_last().expect("fields");
                return Err(format!("Pass at least one of {} or {last}.", rest.join(", ")));
            }
            let update = Update {
                key: text(&input, "key").unwrap_or_default(),
                title: text(&input, "title"),
                description: text(&input, "description"),
                status: text(&input, "status"),
                priority: text(&input, "priority"),
                branch: text(&input, "branch"),
                pr: text(&input, "pr"),
                commit: text(&input, "commit"),
                rel: relations_of(&input),
            };
            let (_, written) = edit(&context, Op::Update(update)).await?;
            let issue = written.issue;
            Ok(if written.changed { format!("Updated {}: {} [{}].", issue.key, issue.title, issue.status.as_str()) } else { format!("{} already has that. Nothing changed.", issue.key) })
        }
        "log" => {
            let (_, written) = edit(&context, Op::Note { key: text(&input, "key").unwrap_or_default(), text: text(&input, "text").unwrap_or_default() }).await?;
            Ok(format!("Logged on {}.", written.issue.key))
        }
        other => Err(format!("The plugin has no handler for {other}")),
    }
}

/// The context a tool call has: the terminal, its project and the agent session in it.
#[cfg(test)]
pub fn context_of(project: Value, session: Value) -> Value {
    serde_json::json!({ "paneId": "p1", "project": project, "session": session })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kv::memory, storage::slot_of};
    use serde_json::json;
    use wings_plugin::task::block_on;

    fn web() -> Value {
        json!({ "id": "proj1", "name": "web", "path": "/work/web" })
    }
    fn agent_one() -> Value {
        json!({ "id": "s-1", "name": "agent one" })
    }

    /// A call as Claude makes it: checked against the tool's schema, then run.
    pub fn call(name: &str, input: Value) -> Result<String, String> {
        call_in(name, input, context_of(web(), agent_one()))
    }

    pub fn call_in(name: &str, input: Value, context: Value) -> Result<String, String> {
        let (_, schema) = schemas().into_iter().find(|(tool, _)| tool == name).unwrap();
        tools::check(&schema, &input, "", name)?;
        block_on(run(name.to_owned(), input, context))
    }

    #[test]
    fn the_manifest_tools_tell_agents_to_link_by_hand_and_keep_keys_out_of_git_and_git_is_only_read_for_a_file_diff() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        let text = |name: &str| m["contributes"]["mcpTools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap().to_owned();
        for name in ["create", "update", "log"] {
            assert!(text(name).contains("Issue keys are local to Wings: never put them in commit messages, branch names, pull request titles or descriptions"), "{name}");
        }
        for part in ["set commit only for one you made in another repo", "set pr to its URL", "set branch when you start on a branch"] {
            assert!(text("update").contains(part), "{part}");
        }
        let mut permissions: Vec<&str> = m["permissions"].as_object().unwrap().keys().map(String::as_str).collect();
        permissions.sort_unstable();
        assert_eq!(permissions, ["events", "exec", "openUrl", "panes"]);
        // A linked pull request's state, the PR of a branch, and the diff of a changed file you click; commits come from the journal, never from git.
        let exec: Vec<&str> = m["permissions"]["exec"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(exec, ["gh pr view [--json --jq]", "az repos pr show [--id --organization --query --output]", "git diff [-- --no-color --no-ext-diff]", "git remote get-url []", "gh pr list [--head --state --limit --json]", "az repos pr list [--organization --project --repository --source-branch --status --top --output]"]);
        assert_eq!(m["permissions"]["events"], json!(["pr-tracker pr-state"]));
    }

    #[test]
    fn the_manifests_instructions_line_sends_claude_to_the_tools_unprompted_and_every_frame_is_a_module() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        let line = m["contributes"]["mcpInstructions"].as_str().unwrap();
        assert!(line.chars().count() <= 500);
        for name in ["create", "list", "get", "update", "log"] {
            assert!(line.contains(&format!("tasks__{name}")), "{name}");
        }
        for part in ["For work that changes files, even unasked", "A question, check or review gets no issue", "deciding something gets no log note"] {
            assert!(line.contains(part), "{part}");
        }
        assert_eq!(m["contributes"]["hooks"], json!(["SessionStart", "PreToolUse", "UserPromptSubmit", "Journal"]));
        assert_eq!(m["main"], "dist/native/main.wasm");
        assert_eq!(m["contributes"]["settings"]["page"], "dist/native/settings.wasm");
        assert!(m["contributes"]["mcpTools"].as_array().unwrap().iter().find(|t| t["name"] == "list").unwrap()["inputSchema"]["properties"]["query"].is_object());
    }

    #[test]
    fn every_tool_in_the_manifest_has_a_handler_and_a_schema_the_module_can_check() {
        let names: Vec<String> = schemas().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["list", "get", "create", "update", "log"]);
        assert!(!MANIFEST.is_empty() && schemas().iter().all(|(_, schema)| schema["type"] == "object"));
        assert!(block_on(run("nope".into(), json!({}), json!({}))).unwrap_err().contains("no handler"));
    }

    #[test]
    fn a_tools_input_is_checked_against_its_schema_first_and_the_error_names_the_field_and_what_it_allows() {
        memory::reset(&[]);
        let wrong: [(&str, Value, &str); 23] = [
            ("get", json!({}), "key is required"),
            ("get", json!({ "key": "" }), "key needs at least 1 character"),
            ("get", json!({ "key": "A", "extra": 1 }), "Unknown field: extra"),
            ("get", json!({ "key": "W".repeat(21) }), "key is limited to 20 characters"),
            ("get", json!(null), "must be an object"),
            ("create", json!({ "title": "" }), "title needs at least 1 character"),
            ("create", json!({ "title": 5 }), "title must be text"),
            ("create", json!({ "title": "x".repeat(201) }), "title is limited to 200 characters, this one has 201"),
            ("create", json!({ "title": "ok", "description": "x".repeat(20_001) }), "description is limited to 20,000"),
            ("create", json!({ "title": "t", "status": "wip" }), "status must be one of todo, in_progress, in_review, done, canceled"),
            ("create", json!({ "title": "t", "priority": "asap" }), "priority must be one of urgent, high, medium, low, none"),
            ("create", json!({ "title": "t", "type": "story" }), "type must be one of task, bug, feature, epic"),
            ("update", json!({ "key": "WEB-1" }), "Pass at least one of title, description, status, priority, branch, pr, commit, type, parent, related_add, related_remove, blocked_by_add or blocked_by_remove."),
            ("update", json!({ "key": "WEB-1", "related_add": "WEB-2" }), "related_add must be a list"),
            ("update", json!({ "key": "WEB-1", "blocked_by_add": [3] }), "blocked_by_add[0] must be text"),
            ("update", json!({ "key": "WEB-1", "related_add": vec!["WEB-2"; 21] }), "related_add takes up to 20 items, this one has 21"),
            ("update", json!({ "key": "WEB-1", "parent": 4 }), "parent must be text"),
            ("update", json!({ "key": "WEB-1", "commit": "a".repeat(41) }), "commit is limited to 40 characters"),
            ("log", json!({ "key": "WEB-1", "text": "x".repeat(20_001) }), "text is limited to 20,000 characters"),
            ("list", json!({ "status": "x" }), "status must be one of"),
            ("list", json!({ "type": "x" }), "type must be one of"),
            ("list", json!({ "query": 5 }), "query must be text"),
            ("list", json!({ "query": "x".repeat(201) }), "query is limited to 200 characters"),
        ];
        for (name, input, message) in wrong {
            let error = call(name, input.clone()).unwrap_err();
            assert!(error.contains(message), "{name} {input}: {error}");
        }
        assert!(memory::keys().is_empty(), "nothing was written");
    }

    #[test]
    fn tools_write_as_the_claude_session_in_the_calling_pane_and_say_what_they_did() {
        memory::reset(&[]);
        assert!(call("create", json!({ "title": "Do it", "description": "Goal", "status": "in_progress" })).unwrap().starts_with("Created WEB-1 in web."));
        assert_eq!(call("log", json!({ "key": "web-1", "text": "next: tests" })).unwrap(), "Logged on WEB-1.");
        let got = call("get", json!({ "key": " web-1 " })).unwrap();
        for part in ["WEB-1 [in_progress] (task) Do it", "Goal", "s-1", "agent one", "agent one note: next: tests"] {
            assert!(got.contains(part), "{part} in {got}");
        }
        assert_eq!(call("update", json!({ "key": "WEB-1", "status": "done" })).unwrap(), "Updated WEB-1: Do it [done].");
        assert_eq!(call("update", json!({ "key": "WEB-1", "status": "done" })).unwrap(), "WEB-1 already has that. Nothing changed.");
        assert!(call("list", json!({ "query": "  do " })).unwrap().starts_with("Issues for web matching \"do\"."));
        assert!(call("log", json!({ "key": "WEB-1", "text": "  " })).unwrap_err().contains("text is required"));
        assert!(call("get", json!({ "key": "web-9" })).unwrap_err().contains("No issue WEB-9 in web"));
        let stored = memory::peek(&format!("issues/{}/WEB-1", slot_of("proj1"))).unwrap();
        assert_eq!(stored["log"][1]["by"], json!({ "id": "s-1", "name": "agent one" }));
    }

    #[test]
    fn a_bad_commit_is_refused_by_the_operation_since_the_schema_has_no_pattern_here() {
        memory::reset(&[]);
        call("create", json!({ "title": "A" })).unwrap();
        assert!(call("update", json!({ "key": "WEB-1", "commit": "xyz" })).unwrap_err().contains("commit must be a git commit hash"));
        assert!(call("update", json!({ "key": "WEB-1", "commit": "ABCDEF1" })).is_ok());
    }

    #[test]
    fn another_project_is_read_by_name_or_path_and_a_call_from_nowhere_says_so() {
        memory::reset(&[]);
        host::testing::answer(|method, _| match method {
            "projects" => Ok(json!([{ "id": "proj1", "name": "web", "path": "/work/web" }, { "id": "proj2", "name": "api", "path": "/work/api" }])),
            _ => Ok(Value::Null),
        });
        call_in("create", json!({ "title": "In api" }), context_of(json!({ "id": "proj2", "name": "api", "path": "/work/api" }), agent_one())).unwrap();
        assert!(call("list", json!({ "project": "API" })).unwrap().contains("API-1 [todo] (task) In api"));
        assert!(call("list", json!({ "project": "/work/api/" })).unwrap().starts_with("Issues for api."));
        assert!(call("list", json!({ "project": "nope" })).unwrap_err().starts_with("No project \"nope\" in Wings. The projects are: web, api."));
        assert!(call_in("list", json!({}), json!({ "paneId": null, "project": null, "session": null })).unwrap_err().starts_with("Can't tell which project"));
    }
}
