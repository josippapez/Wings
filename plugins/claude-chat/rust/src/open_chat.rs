//! The `open_chat` tool: a coding agent opens a Claude chat tab in one of the user's projects, under a Claude Code account it
//! names, and either goes on with a session or starts with a prompt, so "hand this over to my other account" is one call.
//! The tab is the chat page (`chat.rs`) opened with the args it already reads: `cwd`, `account`, `resume`, `fork`, `prompt`,
//! and `bring`, which has Wings copy the session's transcript into the picked account, since Claude Code resumes a session
//! only from its own account's folder.

use serde_json::{Value, json};
use wings_plugin::{host, tools};

const MANIFEST: &str = include_str!("../../wings-plugin.json");

/// How much of a string in a transcript entry is read: only the entries' folder matters here.
const CUT: u32 = 4_000;

/// What `resume` says to mean the session of whoever calls.
const CURRENT: &str = "current";

/// The tool's input schema from the manifest.
fn schema() -> Value {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap_or_default();
    manifest["contributes"]["mcpTools"].as_array().and_then(|list| list.iter().find(|tool| tool["name"] == "open_chat")).map_or(Value::Null, |tool| tool["inputSchema"].clone())
}

pub fn register() {
    tools::register("open_chat", schema(), run);
}

/// A Claude Code session id: the UUID shape its transcripts are named by.
fn session_id(text: &str) -> bool {
    text.len() == 36 && text.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn text(input: &Value, name: &str) -> Option<String> {
    input[name].as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// The project a name, path or id names, or the caller's own when none is given.
fn project_of<'a>(projects: &'a [Value], wanted: Option<&str>, caller: &Value) -> Result<&'a Value, String> {
    let field = |project: &Value, key: &str| project[key].as_str().unwrap_or_default().to_owned();
    let bare = |path: &str| if path.len() > 1 { path.trim_end_matches('/').to_owned() } else { path.to_owned() };
    let list = || projects.iter().map(|project| field(project, "name")).collect::<Vec<_>>().join(", ");
    let Some(wanted) = wanted else {
        // A worktree's project is the folder of its own; the caller's id is found among the projects either way.
        let id = caller["id"].as_str().or_else(|| caller["parentId"].as_str());
        return id.and_then(|id| projects.iter().find(|project| project["id"] == id)).ok_or_else(|| format!("Give project: this call didn't come from a project's terminal. The projects are: {}.", list()));
    };
    if let Some(found) = projects.iter().find(|project| bare(&field(project, "path")) == bare(wanted) || field(project, "id") == wanted) {
        return Ok(found);
    }
    let named: Vec<&Value> = projects.iter().filter(|project| field(project, "name").to_lowercase() == wanted.to_lowercase()).collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("No project \"{wanted}\" in Wings. The projects are: {}.", list())),
        many => Err(format!("{} projects are named \"{wanted}\". Pass the path of one: {}.", many.len(), many.iter().map(|project| field(project, "path")).collect::<Vec<_>>().join(", "))),
    }
}

/// The account a name or an email names, as `Some(None)` for the default one; or why none does.
fn account_of(choices: &[host::AccountChoice], wanted: &str) -> Result<Option<String>, String> {
    let want = wanted.trim().to_lowercase();
    let found = match want.as_str() {
        "default" | "~" => choices.iter().find(|choice| choice.name.is_none()),
        _ => choices.iter().find(|choice| choice.name.as_deref().is_some_and(|name| name.to_lowercase() == want)).or_else(|| choices.iter().find(|choice| choice.email.as_deref().is_some_and(|email| email.to_lowercase() == want))),
    };
    let label = |choice: &host::AccountChoice| match (&choice.name, &choice.email) {
        (Some(name), Some(email)) => format!("{name} ({email})"),
        (Some(name), None) => name.clone(),
        (None, Some(email)) => format!("default ({email})"),
        (None, None) => "default".to_owned(),
    };
    match found {
        Some(choice) if !choice.signed_in => Err(format!("The account {} isn't signed in. Ask the user to sign in to it in Wings first.", label(choice))),
        Some(choice) => Ok(choice.name.clone()),
        None => Err(format!("No Claude Code account \"{wanted}\". The accounts are: {}.", choices.iter().map(label).collect::<Vec<_>>().join(", "))),
    }
}

/// The folder a session ran in, from its transcript's first entry that has one, or why it can't be read.
async fn session_folder(id: &str) -> Result<String, String> {
    let entries = host::transcript_cut(id, &["user"], crate::session::HISTORY_ALL, CUT).await.map_err(|why| format!("No session {id} in any Claude Code account here: {why}"))?;
    entries.as_array().into_iter().flatten().find_map(|entry| entry["cwd"].as_str().filter(|cwd| !cwd.is_empty()).map(str::to_owned)).ok_or_else(|| format!("No session {id} in any Claude Code account here."))
}

/// The args the chat tab opens with. `account` is `None` for the project's own, `Some(name)` for a named one, where
/// `Some(None)` is the default. A session picked for another account is brought there; a session goes on as a fork unless
/// `fork` is false, since the one that asks is still running in its own terminal.
pub fn tab_args(cwd: &str, account: Option<Option<String>>, resume: Option<&str>, fork: bool, prompt: Option<&str>) -> Value {
    let mut args = json!({ "cwd": cwd });
    if let Some(name) = &account {
        args["account"] = json!({ "name": name });
    }
    if let Some(id) = resume {
        args["resume"] = json!(id);
        args["fork"] = json!(fork);
        args["bring"] = json!(account.is_some());
    }
    if let Some(prompt) = prompt {
        args["prompt"] = json!(prompt);
        // A coding agent wrote it, so Claude Code is told it isn't something the user typed.
        args["relayed"] = json!(true);
    }
    args
}

async fn run(input: Value, context: Value) -> Result<Value, String> {
    let (prompt, wanted_account) = (text(&input, "prompt"), text(&input, "account"));
    let resume = match text(&input, "resume") {
        Some(id) if id == CURRENT => Some(context["session"]["id"].as_str().map(str::to_owned).ok_or("resume \"current\" needs the call to come from a Claude Code session in Wings. Pass the session id.")?),
        other => other,
    };
    if resume.is_some() && prompt.is_some() {
        return Err("Give resume or prompt, not both: a session that goes on has its own conversation. Open it, then send the message with send_to_agent.".into());
    }
    if let Some(id) = resume.as_deref().filter(|id| !session_id(id)) {
        return Err(format!("\"{id}\" isn't a Claude Code session id (a UUID like 3c21b0c5-8113-4f55-9493-fa156c3fa369)."));
    }
    let projects = host::projects().await?;
    let projects = projects.as_array().map_or(&[][..], Vec::as_slice);
    let project = project_of(projects, text(&input, "project").as_deref(), &context["project"])?;
    let account = match wanted_account.as_deref() {
        Some(wanted) => Some(account_of(&host::accounts().await?, wanted)?),
        None => None,
    };
    // A session goes on in the folder it ran in, as the chat's own Resume does; a new chat starts in the project's.
    let cwd = match resume.as_deref() {
        Some(id) => session_folder(id).await?,
        None => project["path"].as_str().unwrap_or_default().to_owned(),
    };
    let (project_id, project_name) = (project["id"].as_str().unwrap_or_default().to_owned(), project["name"].as_str().unwrap_or_default().to_owned());
    let fork = input["fork"] != false;
    let args = tab_args(&cwd, account.clone(), resume.as_deref(), fork, prompt.as_deref());
    let opened = host::open_plugin_tab_in("claude-chat", "chat", &project_id, &args).await.map_err(|why| format!("Wings couldn't open the chat: {why}"))?;
    // Wings 0.2.101 answers the tab's id; an older one answers nothing, and the tab is found in list_sessions.
    let tab = opened["tabId"].as_str().map(str::to_owned);
    let under = match &account {
        Some(Some(name)) => format!(" under the account {name}"),
        Some(None) => " under the default account".to_owned(),
        None => String::new(),
    };
    let what = match (&resume, fork) {
        (Some(id), true) => format!("a fork of session {id}, so the original goes on untouched"),
        (Some(id), false) => format!("session {id}"),
        (None, _) if prompt.is_some() => "a new session that starts with your prompt".to_owned(),
        (None, _) => "a new session".to_owned(),
    };
    let reach = match tab {
        Some(tab) => format!("Its tabId is {tab}: send_to_agent with it sends the chat a message."),
        None => "list_sessions shows it under tabs, and send_to_agent with its tabId sends it a message.".to_owned(),
    };
    Ok(Value::String(format!("Opened a Claude chat tab in {project_name}{under} with {what}, in the background. {reach}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::task::block_on;

    const SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";

    fn projects() -> Value {
        json!([
            { "id": "/work/shop", "name": "shop", "path": "/work/shop" },
            { "id": "/work/api", "name": "api", "path": "/work/api" },
        ])
    }

    fn accounts() -> Value {
        json!([
            { "name": null, "email": "me@work.example", "signedIn": true },
            { "name": "personal", "email": "me@home.example", "signedIn": true },
            { "name": "old", "email": null, "signedIn": false },
        ])
    }

    /// Wings answering the calls the tool makes, and keeping every one.
    fn wings() -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            match method {
                "projects" => Ok(projects()),
                "claudeAccountChoices" => Ok(accounts()),
                "transcript" if params["sessionId"] == SESSION => Ok(json!([{ "type": "user", "cwd": "/work/shop/web" }])),
                "transcript" => Err("no transcript".into()),
                _ => Ok(Value::Null),
            }
        });
        calls
    }

    fn call(input: Value) -> Result<Value, String> {
        block_on(run(input, json!({ "project": { "id": "/work/api" }, "session": { "id": SESSION } })))
    }

    fn opened(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<Value> {
        calls.borrow().iter().filter(|(method, _)| method == "openView").map(|(_, params)| params.clone()).collect()
    }

    #[test]
    fn a_prompt_opens_a_new_chat_in_the_project_under_the_account_named_by_its_email() {
        let calls = wings();
        let said = call(json!({ "project": "shop", "account": "ME@home.example", "prompt": "Take over the retry work" })).unwrap();
        assert!(said.as_str().unwrap().contains("under the account personal"), "{said}");
        let open = opened(&calls);
        assert_eq!(open.len(), 1);
        assert_eq!((open[0]["plugin"].as_str(), open[0]["id"].as_str(), open[0]["projectId"].as_str(), open[0]["newTab"].clone()), (Some("claude-chat"), Some("chat"), Some("/work/shop"), json!(true)));
        assert_eq!(open[0]["args"], json!({ "cwd": "/work/shop", "account": { "name": "personal" }, "prompt": "Take over the retry work", "relayed": true }));
    }

    #[test]
    fn the_default_account_is_picked_with_a_null_name_and_none_named_leaves_the_projects() {
        let calls = wings();
        call(json!({ "account": "default" })).unwrap();
        call(json!({})).unwrap();
        let open = opened(&calls);
        // No project given: the caller's own.
        assert_eq!(open[0]["projectId"], "/work/api");
        assert_eq!(open[0]["args"], json!({ "cwd": "/work/api", "account": { "name": null } }));
        assert_eq!(open[1]["args"], json!({ "cwd": "/work/api" }));
    }

    #[test]
    fn resuming_a_session_on_another_account_forks_it_and_brings_its_transcript_in_the_folder_it_ran_in() {
        let calls = wings();
        call(json!({ "resume": "current", "account": "personal" })).unwrap();
        call(json!({ "resume": SESSION, "account": "personal", "fork": false })).unwrap();
        call(json!({ "resume": SESSION })).unwrap();
        let args: Vec<Value> = opened(&calls).iter().map(|open| open["args"].clone()).collect();
        assert_eq!(args[0], json!({ "cwd": "/work/shop/web", "account": { "name": "personal" }, "resume": SESSION, "fork": true, "bring": true }));
        assert_eq!(args[1]["fork"], false);
        // No account named: nothing to bring.
        assert_eq!((args[2]["bring"].clone(), args[2].get("account")), (json!(false), None));
    }

    #[test]
    fn what_cannot_be_opened_comes_back_as_an_error_that_says_why_and_opens_nothing() {
        let calls = wings();
        let fails = |input: Value, part: &str| {
            let why = call(input).unwrap_err();
            assert!(why.contains(part), "{part:?} in {why:?}");
        };
        fails(json!({ "account": "nobody" }), "No Claude Code account \"nobody\". The accounts are: default (me@work.example), personal (me@home.example), old");
        fails(json!({ "account": "old" }), "isn't signed in");
        fails(json!({ "project": "nope" }), "No project \"nope\" in Wings. The projects are: shop, api.");
        fails(json!({ "resume": "55555555-2222-4333-8444-555555555555" }), "No session 55555555-2222-4333-8444-555555555555");
        fails(json!({ "resume": "../etc" }), "isn't a Claude Code session id");
        fails(json!({ "resume": SESSION, "prompt": "hi" }), "resume or prompt, not both");
        assert!(opened(&calls).is_empty());
        // "current" from a call that isn't a Claude Code session.
        let why = block_on(run(json!({ "resume": "current" }), json!({ "project": { "id": "/work/api" } }))).unwrap_err();
        assert!(why.contains("needs the call to come from a Claude Code session"), "{why}");
        // No project given and the call isn't from one.
        let why = block_on(run(json!({}), json!({}))).unwrap_err();
        assert!(why.starts_with("Give project"), "{why}");
    }

    #[test]
    fn the_manifest_declares_the_tool_with_the_inputs_it_reads() {
        let schema = schema();
        let names: Vec<&str> = schema["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        for name in ["project", "account", "resume", "prompt", "fork"] {
            assert!(names.contains(&name), "{name}");
        }
        assert_eq!(schema["additionalProperties"], false);
    }
}
