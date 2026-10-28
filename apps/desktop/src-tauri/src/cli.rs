//! `wings plugin ...`: install and manage plugins from a terminal or a script. It talks to the running Wings
//! over its socket, so the app's list, the Plugins sheet and Claude's tools stay in step. `wings statusline`
//! passes Claude Code's status line data on the same way, and `wings hook` answers its hooks.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

/// `identifier` in tauri.conf.json. Tauri names the app's data folder after it.
const IDENTIFIER: &str = "dev.wings.app";

const USAGE: &str = "Usage: wings plugin <command>

  list [--json]           Installed plugins and whether they're on
  install <file | link>   Install a .wings-plugin file or a GitHub repo, like github.com/owner/name
                          or a plugin folder in one, like github.com/owner/name/plugins/<id>
  link <folder>           Run a plugin folder where it is, which `wings-plugin dev` does after each build
  update <id>             Reinstall a GitHub plugin from its newest release
  enable <id>             Turn a plugin on. New access still needs your approval in Wings
  disable <id>            Turn a plugin off
  remove <id>             Uninstall a plugin and delete its saved secrets

Wings has to be open.";

pub fn run(args: &[String]) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (op, request) = match args.as_slice() {
        ["list"] | ["list", "--json"] => ("plugins", json!({})),
        ["install", source] => {
            let path = Path::new(source);
            // The app runs elsewhere, so a file goes as an absolute path.
            let source = if path.exists() {
                match path.canonicalize() {
                    Ok(full) => full.to_string_lossy().into_owned(),
                    Err(e) => return fail(&format!("Couldn't read {source}: {e}")),
                }
            } else {
                source.to_string()
            };
            ("install", json!({ "source": source }))
        }
        ["link", folder] => match Path::new(folder).canonicalize() {
            Ok(full) => ("link", json!({ "folder": full.to_string_lossy() })),
            Err(e) => return fail(&format!("Couldn't read {folder}: {e}")),
        },
        [op @ ("update" | "enable" | "disable" | "remove"), id] => (*op, json!({ "plugin": id })),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let reply = match ask(op, request) {
        Ok(reply) => reply,
        Err(e) => return fail(&e),
    };
    match (op, &reply) {
        ("plugins", list) if args.contains(&"--json") => println!("{}", serde_json::to_string_pretty(list).unwrap_or_default()),
        ("plugins", Value::Array(list)) if list.is_empty() => println!("No plugins installed."),
        ("plugins", Value::Array(list)) => {
            for p in list {
                println!("{:<20} {:<10} {:<15} {}", text(p, "id"), text(p, "version"), state(p), source(p));
            }
        }
        ("remove", _) => println!("Removed {}.", args[1]),
        (_, view) => {
            println!("{} {} is {}.", text(view, "name"), text(view, "version"), state(view));
            if state(view) == "waiting for approval" {
                println!("Review its access in Wings to turn it on.");
            }
        }
    }
    0
}

fn fail(message: &str) -> i32 {
    eprintln!("wings: {message}");
    1
}

fn socket() -> Result<PathBuf, String> {
    Ok(dirs::data_dir().ok_or("Couldn't find the app data folder")?.join(IDENTIFIER).join("mcp.sock"))
}

/// One request to the running app.
fn ask(op: &str, mut request: Value) -> Result<Value, String> {
    let stream = UnixStream::connect(socket()?).map_err(|_| "Wings isn't running. Open Wings and try again.".to_string())?;
    request["op"] = json!(op);
    request["id"] = json!(1);
    writeln!(&stream, "{request}").map_err(|e| e.to_string())?;
    // The app also sends events on this connection; skip to the reply.
    for line in BufReader::new(&stream).lines() {
        let message: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?).unwrap_or(Value::Null);
        if message.get("id") != Some(&json!(1)) {
            continue;
        }
        if let Some(error) = message.get("error").and_then(Value::as_str) {
            return Err(error.to_string());
        }
        // `list` answers with its fields at the top, not under `result`.
        return Ok(message.get("result").cloned().unwrap_or(message));
    }
    Err("Wings closed the connection".into())
}

/// A plugin tool's name on the socket: `<plugin>__<tool>`, as Claude sees it.
const SEPARATOR: &str = "__";

/// `wings tools`: the commands the plugins that are on add, which are their MCP tools, one per line.
pub fn tools() -> i32 {
    let reply = match ask("list", json!({})) {
        Ok(reply) => reply,
        Err(e) => return fail(&e),
    };
    let mut rows: Vec<(String, String)> = reply["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let (plugin, tool) = t["name"].as_str()?.split_once(SEPARATOR)?;
            let first = t["description"].as_str().unwrap_or_default().split(". ").next().unwrap_or_default().trim_end_matches('.').to_string();
            Some((format!("wings {plugin} {tool}"), first))
        })
        .collect();
    if rows.is_empty() {
        println!("No plugin that's on adds commands.");
        return 0;
    }
    rows.sort();
    let width = rows.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
    for (command, about) in rows {
        println!("{command:<width$}  {about}");
    }
    println!("\nRun one with key=value arguments, like `wings tasks list status=todo`. `wings <plugin> <tool> --help` shows its arguments.");
    0
}

/// A `key=value` argument as JSON: a number, true, false or a list as such, anything else as text.
fn argument(value: &str) -> Value {
    match serde_json::from_str::<Value>(value) {
        Ok(v @ (Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Object(_))) => v,
        _ => json!(value),
    }
}

/// `wings <plugin> <tool> [key=value ...]`: runs a plugin's tool, the one Claude can call as `<plugin>__<tool>`, and
/// prints its answer. Exits 1 when the tool says it failed.
pub fn plugin_tool(args: &[String]) -> i32 {
    let [plugin, tool, rest @ ..] = args else {
        eprintln!("Usage: wings <plugin> <tool> [key=value ...]. `wings tools` lists them.");
        return 2;
    };
    let name = format!("{plugin}{SEPARATOR}{tool}");
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        let reply = match ask("list", json!({})) {
            Ok(reply) => reply,
            Err(e) => return fail(&e),
        };
        let Some(found) = reply["tools"].as_array().into_iter().flatten().find(|t| t["name"] == json!(name)) else {
            return fail(&format!("{plugin} has no command {tool}, or it's off. `wings tools` lists them."));
        };
        println!("{}\n", found["description"].as_str().unwrap_or_default());
        let required: Vec<&str> = found["inputSchema"]["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        for (key, schema) in found["inputSchema"]["properties"].as_object().into_iter().flatten() {
            let kind = schema["type"].as_str().unwrap_or("value");
            let need = if required.contains(&key.as_str()) { " (required)" } else { "" };
            println!("  {key}=<{kind}>{need}  {}", schema["description"].as_str().unwrap_or_default());
        }
        return 0;
    }
    call_tool(&name, rest)
}

/// `wings worktree-setup [project=<name|path>] [include='["a","b"]'] [setup="..."]`: Wings' own `worktree_setup`
/// tool. With neither include nor setup it prints the project's settings; with them it changes them.
pub fn worktree_setup(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wings worktree-setup [project=<name or folder>] [include='[\"pattern\", ...]'] [setup=\"shell line\"]\n\nWithout include and setup it prints what the project's new worktrees get. include replaces the list of gitignored files copied in (an empty list resets it to the default; patterns use .gitignore syntax, ! leaves files out). setup is a shell line run in a new worktree before its agent starts (empty clears it). project defaults to the project of the pane you run in.");
        return 0;
    }
    call_tool("worktree_setup", args)
}

/// Calls the tool `name` with `key=value` arguments and prints its answer. Exits 1 when the tool says it failed.
fn call_tool(name: &str, rest: &[String]) -> i32 {
    let mut arguments = serde_json::Map::new();
    for arg in rest {
        let Some((key, value)) = arg.split_once('=') else {
            return fail(&format!("{arg} isn't key=value"));
        };
        // A project like `2024` is a name, not a number.
        arguments.insert(key.to_string(), if key == "project" { json!(value) } else { argument(value) });
    }
    let request = json!({ "name": name, "arguments": arguments, "ppid": std::os::unix::process::parent_id() });
    match ask("call", request) {
        Ok(result) => {
            let text = result.pointer("/content/0/text").and_then(Value::as_str).unwrap_or_default();
            if result["isError"] == true {
                return fail(text);
            }
            println!("{text}");
            0
        }
        Err(e) => fail(&e),
    }
}

/// Claude Code's status JSON is a few KB.
const STATUS_LIMIT: u64 = 256 * 1024;

/// `wings statusline`, for Claude Code's `statusLine` setting. Passes the status JSON Claude Code writes to stdin
/// on to the running Wings, for plugins to show. With `--pass` it also prints that input for the status line
/// you had before Wings, which runs after it; without, it prints nothing. Without Wings open it only does that.
pub fn statusline(pass: bool) -> i32 {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("wings statusline is Claude Code's statusLine command: it reads the status Claude Code sends it.");
        return 2;
    }
    let mut input = Vec::new();
    if stdin.lock().take(STATUS_LIMIT).read_to_end(&mut input).is_err() {
        return 0;
    }
    if pass {
        // Your status line first, so Wings never holds it up.
        let mut out = std::io::stdout();
        let _ = out.write_all(&input).and_then(|()| out.flush());
    }
    let Ok(status) = serde_json::from_slice::<Value>(&input) else { return 0 };
    if let Ok(stream) = socket().and_then(|s| UnixStream::connect(s).map_err(|e| e.to_string())) {
        // Claude Code waits for this command, so a stuck Wings mustn't hold it up.
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(1)));
        let _ = writeln!(&stream, "{}", json!({ "op": "statusline", "id": 1, "status": status }));
    }
    0
}

/// A new tab may still be loading its shell when the command is typed, so `wait` gives it this long to start.
const START_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// `wings wait <pane>`: returns once the pane's command has finished and its shell is back at the prompt, then
/// prints the end of its output. Claude runs it in the background after `open_pane`, and Claude Code tells it
/// when it exits.
pub fn wait(args: &[String]) -> i32 {
    let [pane] = args else {
        eprintln!("Usage: wings wait <pane id>");
        return 2;
    };
    let mut last = Value::Null;
    let busy = || {
        ask("pane_busy", json!({ "pane": pane })).map(|r| {
            let busy = r["busy"] == true;
            last = r;
            busy
        })
    };
    if let Err(e) = wait_until_done(busy, std::time::Duration::from_secs(1), START_GRACE) {
        return fail(&e);
    }
    if last["agent"] == true {
        println!("The agent in pane {pane} finished its turn.");
        if let Some(id) = last["sessionId"].as_str() {
            println!("Read its answer with read_session, session_id {id}, or the end of its screen with read_pane.");
        }
        return 0;
    }
    let output = ask("call", json!({ "name": "read_pane", "arguments": { "pane_id": pane, "lines": 30 } }));
    println!("Pane {pane} finished its command. Close it with close_pane once you're done with its output.");
    if let Some(text) = output.ok().as_ref().and_then(|r| r.pointer("/content/0/text")).and_then(Value::as_str) {
        println!("\n{text}");
    }
    0
}

/// Polls `busy` until the command has started and ended, or never started within `grace`.
fn wait_until_done(mut busy: impl FnMut() -> Result<bool, String>, poll: std::time::Duration, grace: std::time::Duration) -> Result<(), String> {
    let since = std::time::Instant::now();
    let mut started = false;
    loop {
        match busy()? {
            true => started = true,
            false if started || since.elapsed() >= grace => return Ok(()),
            false => {}
        }
        std::thread::sleep(poll);
    }
}

/// A hook input is the session, the tool and its input: a Bash command can be long, a file's contents aren't in it.
const HOOK_LIMIT: u64 = 1024 * 1024;
/// Claude Code's own cap on a hook's `additionalContext`.
const CONTEXT_LIMIT: usize = 10_000;
/// How long `wings hook` waits for Wings, in all, before it says nothing.
const HOOK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// `wings hook`, for Claude Code's `SessionStart`, `PreToolUse` and `UserPromptSubmit` hooks. Hands the hook's input to the running
/// Wings, which asks the plugins that answer that event, and prints what they said as Claude Code expects it.
/// It never holds Claude up: with Wings closed, slow, or with nothing to say, it prints nothing and exits 0.
pub fn hook() -> i32 {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("wings hook is a Claude Code hook command: it reads the hook input Claude Code sends it.");
        return 2;
    }
    let mut input = Vec::new();
    if stdin.lock().take(HOOK_LIMIT).read_to_end(&mut input).is_err() {
        return 0;
    }
    let Ok(input) = serde_json::from_slice::<Value>(&input) else { return 0 };
    let Some(reply) = hook_reply(&input) else { return 0 };
    if let Some(out) = hook_output(&input, &reply) {
        println!("{out}");
    }
    0
}

/// `wings journal`, the sink for an agent's per-turn change journal (Claude Code or OpenCode): reads one JSON object
/// on stdin (`session_id`, `cwd`, `agent`, `prompt`, `files`, `more`, `commits`) and hands it to the running Wings as
/// a `Journal` hook input, so plugins that answer `Journal` can note the turn on the session's issue. It is not a
/// Claude Code hook and isn't registered in its settings. It prints nothing and always exits 0.
pub fn journal() -> i32 {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("wings journal is a sink command: it reads the journal's JSON on stdin.");
        return 2;
    }
    let mut input = Vec::new();
    if stdin.lock().take(HOOK_LIMIT).read_to_end(&mut input).is_err() {
        return 0;
    }
    if let Some(input) = journal_input(&input) {
        let _ = hook_reply(&input);
    }
    0
}

/// The journal's payload as a hook input, or nothing when it isn't a JSON object.
fn journal_input(payload: &[u8]) -> Option<Value> {
    let Value::Object(mut fields) = serde_json::from_slice::<Value>(payload).ok()? else { return None };
    fields.insert("hook_event_name".into(), json!("Journal"));
    Some(Value::Object(fields))
}

/// Wings' answer to a hook input, within `HOOK_WAIT`.
fn hook_reply(input: &Value) -> Option<Value> {
    let start = std::time::Instant::now();
    let stream = UnixStream::connect(socket().ok()?).ok()?;
    stream.set_write_timeout(Some(HOOK_WAIT)).ok()?;
    let request = json!({ "op": "hook", "id": 1, "input": input, "ppid": std::os::unix::process::parent_id() });
    writeln!(&stream, "{request}").ok()?;
    let mut lines = BufReader::new(&stream);
    loop {
        // One deadline for the whole wait, not for each line: Wings also sends events on this connection.
        let left = HOOK_WAIT.checked_sub(start.elapsed())?;
        stream.set_read_timeout(Some(left)).ok()?;
        let mut line = String::new();
        if lines.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message.get("id") == Some(&json!(1)) {
            return message.get("result").cloned();
        }
    }
}

/// What Claude Code's hook JSON says for Wings' reply `{ context?, deny? }`, or nothing when the reply says nothing.
fn hook_output(input: &Value, reply: &Value) -> Option<Value> {
    let event = input.get("hook_event_name").and_then(Value::as_str)?;
    let say = |key: &str| reply.get(key).and_then(Value::as_str).filter(|t| !t.is_empty());
    let mut out = serde_json::Map::new();
    out.insert("hookEventName".into(), json!(event));
    if let Some(context) = say("context") {
        out.insert("additionalContext".into(), json!(context.chars().take(CONTEXT_LIMIT).collect::<String>()));
    }
    if let (Some(reason), "PreToolUse") = (say("deny"), event) {
        out.insert("permissionDecision".into(), json!("deny"));
        out.insert("permissionDecisionReason".into(), json!(reason));
    }
    (out.len() > 1).then(|| json!({ "hookSpecificOutput": out }))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn state(p: &Value) -> &'static str {
    let on = |key: &str| p.get(key).and_then(Value::as_bool).unwrap_or(false);
    match (on("enabled"), on("approved")) {
        (true, true) => "on",
        (false, true) => "off",
        _ => "waiting for approval",
    }
}

fn source(p: &Value) -> String {
    if let Some(folder) = p["folder"].as_str() {
        return format!("linked from {folder}");
    }
    if p.get("dev").and_then(Value::as_bool).unwrap_or(false) {
        return "this repo".into();
    }
    match p["source"]["kind"].as_str() {
        Some("github") => {
            let folder = p["source"]["folder"].as_str().map(|f| format!("/{f}")).unwrap_or_default();
            format!("github.com/{}{folder}", p["source"]["repo"].as_str().unwrap_or_default())
        }
        _ => "file".into(),
    }
}

// ---------- putting `wings` on PATH ----------

const MARKER: &str = "# The wings command, added by Wings.";

/// ~/.local/bin needs no admin rights, and many setups already have it on PATH (Claude Code's installer adds it).
pub fn command_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".local/bin/wings"))
}

/// A script that runs this copy of Wings. With no arguments it opens the app, rather than starting a second one.
pub fn command_script(exe: &Path) -> String {
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"));
    let bundle = exe.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app"));
    let open = bundle.map(|b| format!("[ $# -eq 0 ] && exec open {}\n", quote(b))).unwrap_or_default();
    format!("#!/bin/sh\n{MARKER} With no arguments it opens the app.\n{open}exec {} \"$@\"\n", quote(exe))
}

/// Writes the command, unless something that isn't from Wings is already there.
pub fn install_command(exe: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let path = command_path().ok_or("Couldn't find your home folder")?;
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if !existing.contains(MARKER) {
            return Err(format!("{} already exists and isn't from Wings. Remove it, then try again.", path.display()));
        }
    }
    let dir = path.parent().ok_or("Couldn't find the folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, command_script(exe)).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_command_argument_reads_as_json_when_it_is_a_number_bool_or_list_else_as_text() {
        use super::argument;
        assert_eq!(argument("5"), json!(5));
        assert_eq!(argument("true"), json!(true));
        assert_eq!(argument(r#"["New","Ready"]"#), json!(["New", "Ready"]));
        assert_eq!(argument("todo"), json!("todo"));
        assert_eq!(argument("\"quoted\""), json!("\"quoted\""), "a JSON string stays the text as typed");
        assert_eq!(argument(""), json!(""));
    }

    use serde_json::json;

    #[test]
    fn wait_returns_once_a_started_command_ends_or_one_never_starts() {
        use std::time::Duration;
        let polls = |states: Vec<Result<bool, String>>, grace: Duration| {
            let mut states = states.into_iter();
            let mut count = 0;
            let result = super::wait_until_done(|| { count += 1; states.next().unwrap() }, Duration::ZERO, grace);
            (result, count)
        };
        // A shell still loading isn't the command finishing.
        assert_eq!(polls(vec![Ok(false), Ok(true), Ok(true), Ok(false)], Duration::from_secs(60)), (Ok(()), 4));
        assert_eq!(polls(vec![Ok(false)], Duration::ZERO), (Ok(()), 1));
        assert_eq!(polls(vec![Ok(true), Err("No pane p9".into())], Duration::from_secs(60)), (Err("No pane p9".into()), 2));
    }

    #[test]
    fn hook_output_is_what_claude_code_reads_or_nothing() {
        let start = json!({ "hook_event_name": "SessionStart" });
        assert_eq!(super::hook_output(&start, &json!({})), None);
        assert_eq!(super::hook_output(&start, &json!({ "context": "" })), None);
        assert_eq!(
            super::hook_output(&start, &json!({ "context": "use the tracker" })),
            Some(json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": "use the tracker" } }))
        );
        // Only PreToolUse can refuse.
        assert_eq!(super::hook_output(&start, &json!({ "deny": "no" })), None);
        let pre = json!({ "hook_event_name": "PreToolUse" });
        assert_eq!(
            super::hook_output(&pre, &json!({ "deny": "no", "context": "why" })),
            Some(json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "additionalContext": "why", "permissionDecision": "deny", "permissionDecisionReason": "no" } }))
        );
        let long = "x".repeat(super::CONTEXT_LIMIT + 5);
        let out = super::hook_output(&start, &json!({ "context": long })).unwrap();
        assert_eq!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().len(), super::CONTEXT_LIMIT);
    }

    #[test]
    fn the_journal_payload_becomes_a_journal_hook_input() {
        let input = super::journal_input(br#"{ "session_id": "s1", "cwd": "/r", "agent": "opencode", "files": [{ "path": "a.ts", "what": "x" }], "commits": ["abc123d"] }"#).unwrap();
        assert_eq!(input["hook_event_name"], "Journal");
        assert_eq!(input["session_id"], "s1");
        assert_eq!(input["files"][0]["path"], "a.ts");
        assert_eq!(input["commits"], json!(["abc123d"]));
        // Whatever the payload says for the event, it is a Journal.
        assert_eq!(super::journal_input(br#"{ "hook_event_name": "PreToolUse" }"#).unwrap()["hook_event_name"], "Journal");
        assert_eq!(super::journal_input(b"not json"), None);
        assert_eq!(super::journal_input(b"[1]"), None);
    }

    #[test]
    fn command_opens_the_app_with_no_arguments() {
        let script = super::command_script(std::path::Path::new("/Applications/Wing's.app/Contents/MacOS/wings"));
        assert!(script.contains(r"exec open '/Applications/Wing'\''s.app'"), "{script}");
        assert!(script.ends_with("exec '/Applications/Wing'\\''s.app/Contents/MacOS/wings' \"$@\"\n"), "{script}");
        let plain = super::command_script(std::path::Path::new("/opt/wings/wings"));
        assert!(!plain.contains("exec open"), "{plain}");
    }

    #[test]
    fn identifier_matches_the_tauri_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], super::IDENTIFIER);
    }
}
