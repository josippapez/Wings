//! `wings --mcp <socket>`: the MCP server Claude Code starts over stdio, registered once at user scope.
//! It speaks MCP to Claude and relays tool listing and calls to the running Wings app over its socket.
//! It outlives Wings restarts by reconnecting, and tells Claude to refetch tools when they change.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};

/// Answered when Claude asks for one we know, else our newest. Claude Code accepts these on both of its
/// MCP runtimes; on the 2026-07-28 revision `list_changed` would need a held-open stream instead.
const PROTOCOLS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const INSTRUCTIONS: &str = "Tools from Wings, the terminal app this session may run in. search_history and read_session \
find and read past Claude Code and OpenCode sessions on this computer. list_sessions, read_pane, open_pane, close_pane, send_to_agent and notify see and use \
Wings' open terminal panes, and let you message the agents in them. Run anything that takes more than a few seconds (a dev server, \
a watcher, a long build or test run) with open_pane and run, not Bash in the background, so the user sees it in a tab and can \
stop it. Read its output with read_pane. To hear when it finishes, run the waitCommand from open_pane's reply with Bash in \
the background instead of polling. Close a tab you opened with close_pane once you're done with it, unless the user \
should keep it, like a dev server they're using. Keep Bash for quick commands. Tools from the plugins turned on in Wings are named \
<plugin>__<tool>, like tasks__list. More Wings commands run through Bash: `wings tools` lists them, for example \
`wings disk folder_usage path=…`, `wings pr-tracker failing_checks`, `wings call create_worktree branch=…` (not git worktree add). \
With Wings closed there are none.";
/// Claude Code cuts a server's instructions at this many characters.
const MAX_INSTRUCTIONS: usize = 4096;
/// How long `initialize` waits for Wings to hand over its plugins' lines before answering with the static text.
/// Longer than the longest ordinary wait inside Wings: a worktree_setup change waits up to 5 minutes for the user.
const TOOL_WAIT: Duration = Duration::from_secs(310);
/// Approval can take five minutes, then setup_worktree can run its command for fifteen more.
const SETUP_WORKTREE_WAIT: Duration = Duration::from_secs(1_210);
const INSTRUCTIONS_WAIT: Duration = Duration::from_secs(2);

struct Bridge {
    /// Where the MCP messages to Claude Code go: stdout.
    stdout: Mutex<Box<dyn Write + Send>>,
    app: Mutex<Option<UnixStream>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    next: AtomicU64,
    initialized: AtomicBool,
    /// The Wings version the server reports.
    version: &'static str,
    /// How long each tool that may take longer than `TOOL_WAIT` may take, from the last tool list (`maxSeconds`).
    waits: Mutex<HashMap<String, Duration>>,
}

impl Bridge {
    fn send(&self, message: Value) {
        let mut out = self.stdout.lock().unwrap();
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn tools_changed(&self) {
        if self.initialized.load(Ordering::Relaxed) {
            self.send(json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
        }
    }

    /// One request to the Wings app; `None` when Wings isn't running or doesn't answer in time.
    fn ask(&self, mut request: Value, timeout: Duration) -> Option<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        request["id"] = json!(id);
        let (done, answer) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, done);
        let sent = self.app.lock().unwrap().as_mut().is_some_and(|app| writeln!(app, "{request}").is_ok());
        let reply = if sent { answer.recv_timeout(timeout).ok() } else { None };
        self.pending.lock().unwrap().remove(&id);
        reply
    }
}

/// The static text, then each plugin's line while it still fits under Claude Code's cap. A line that
/// doesn't fit whole is dropped rather than cut mid-sentence.
fn instructions(plugin_lines: &[&str]) -> String {
    let mut text = INSTRUCTIONS.to_string();
    for line in plugin_lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if text.chars().count() + 1 + line.chars().count() > MAX_INSTRUCTIONS {
            continue;
        }
        text.push('\n');
        text.push_str(line);
    }
    text
}

/// The tools Claude Code is told of: the app's list without the ones it marks `cliOnly`, which stay `wings` commands, and
/// without what only Wings reads (`maxSeconds`). A `tools/call` of one still runs it.
fn listed(tools: Option<Value>) -> Value {
    let Some(Value::Array(tools)) = tools else { return json!([]) };
    let shown = tools.into_iter().filter(|t| t["cliOnly"] != true).map(|mut tool| {
        if let Some(fields) = tool.as_object_mut() {
            fields.remove("maxSeconds");
        }
        tool
    });
    Value::Array(shown.collect())
}

/// How long a call of each tool in `tools` that waits on the person (`maxSeconds`) may take, a little over what Wings
/// gives it, so Wings' own answer that it ran out of time comes through.
fn waits(tools: &Option<Value>) -> HashMap<String, Duration> {
    let listed = tools.as_ref().and_then(Value::as_array).into_iter().flatten();
    listed
        .filter_map(|tool| Some((tool["name"].as_str()?.to_owned(), Duration::from_secs(tool["maxSeconds"].as_u64()? + 10))))
        .collect()
}

/// How long `bridge` waits for Wings to answer a call of `name`.
fn call_wait(waits: &HashMap<String, Duration>, name: Option<&str>) -> Duration {
    match name {
        Some("setup_worktree") => SETUP_WORKTREE_WAIT,
        Some(name) => waits.get(name).copied().map_or(TOOL_WAIT, |wait| wait.max(TOOL_WAIT)),
        None => TOOL_WAIT,
    }
}

/// `version` is the Wings version the server reports to Claude Code.
pub fn run(socket: Option<String>, version: &'static str) -> i32 {
    let Some(socket) = socket.map(PathBuf::from) else {
        eprintln!("usage: wings --mcp <socket>");
        return 2;
    };
    let bridge = Arc::new(Bridge {
        stdout: Mutex::new(Box::new(std::io::stdout())),
        app: Mutex::new(None),
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        initialized: AtomicBool::new(false),
        version,
        waits: Mutex::new(HashMap::new()),
    });
    link(&bridge, socket);
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let bridge = bridge.clone();
        // Each request on its own thread, so a slow tool call doesn't hold up the others.
        thread::spawn(move || handle(&bridge, message));
    }
    0
}

/// Links the bridge to the Wings app at `socket`, and keeps it linked: it reconnects after Wings quits or restarts.
fn link(bridge: &Arc<Bridge>, socket: PathBuf) {
    // One connect before reading stdin, so `initialize` (Claude's first message) finds Wings already linked
    // instead of racing the thread below. With Wings closed this fails at once.
    let mut first = UnixStream::connect(&socket).ok();
    if let Some(app) = first.as_ref().and_then(|s| s.try_clone().ok()) {
        *bridge.app.lock().unwrap() = Some(app);
    }
    let link = bridge.clone();
    thread::spawn(move || loop {
        if let Some(stream) = first.take().or_else(|| UnixStream::connect(&socket).ok()) {
            if let Ok(reader) = stream.try_clone() {
                *link.app.lock().unwrap() = Some(stream);
                link.tools_changed();
                for line in BufReader::new(reader).lines() {
                    let Ok(message) = line.map(|l| serde_json::from_str::<Value>(&l).unwrap_or(Value::Null)) else { break };
                    if message.get("event").and_then(Value::as_str) == Some("tools_changed") {
                        link.tools_changed();
                    } else if let Some(id) = message.get("id").and_then(Value::as_u64) {
                        if let Some(done) = link.pending.lock().unwrap().remove(&id) {
                            let _ = done.send(message);
                        }
                    }
                }
                *link.app.lock().unwrap() = None;
                link.tools_changed();
            }
        }
        thread::sleep(Duration::from_secs(2));
    });
}

fn handle(bridge: &Bridge, message: Value) {
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let Some(id) = message.get("id").cloned() else {
        if method == "notifications/initialized" {
            bridge.initialized.store(true, Ordering::Relaxed);
        }
        return;
    };
    let result = match method {
        "initialize" => {
            let asked = message["params"]["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
            // Wings closed or slow: the static text alone.
            let reply = bridge.ask(json!({ "op": "list" }), INSTRUCTIONS_WAIT);
            let lines: Vec<&str> = reply.as_ref().and_then(|r| r["instructions"].as_array()).map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_str).collect());
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": "wings", "title": "Wings", "version": bridge.version },
                "instructions": instructions(&lines),
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools = bridge.ask(json!({ "op": "list" }), Duration::from_secs(5)).and_then(|r| r.get("tools").cloned());
            *bridge.waits.lock().unwrap() = waits(&tools);
            Ok(json!({ "tools": listed(tools) }))
        }
        "tools/call" => {
            let request = json!({
                "op": "call",
                "name": message["params"]["name"],
                "arguments": message["params"].get("arguments").cloned().unwrap_or_else(|| json!({})),
                "ppid": std::os::unix::process::parent_id(),
                "sessionId": message["params"]["_meta"]["ai.opencode/sessionID"],
            });
            let timeout = call_wait(&bridge.waits.lock().unwrap(), message["params"]["name"].as_str());
            Ok(match bridge.ask(request, timeout) {
                Some(reply) => reply.get("result").cloned().unwrap_or_else(|| {
                    let error = reply.get("error").and_then(Value::as_str).unwrap_or("Wings couldn't run the tool");
                    json!({ "content": [{ "type": "text", "text": error }], "isError": true })
                }),
                None => json!({ "content": [{ "type": "text", "text": "Wings isn't running. Open Wings to use its tools." }], "isError": true }),
            })
        }
        _ => Err(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
    };
    bridge.send(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_server_calls_keep_the_invoking_session() {
        let (app, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let bridge = Bridge {
            stdout: Mutex::new(Box::new(std::io::sink())),
            app: Mutex::new(Some(app)),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            initialized: AtomicBool::new(false),
            version: "0.0.0",
            waits: Mutex::new(HashMap::new()),
        };
        let sessions = [Some("ses_wings"), Some("ses_setup"), None];
        thread::scope(|scope| {
            let received = scope.spawn(|| {
                let mut reader = BufReader::new(peer);
                sessions.map(|_| {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let id = request["id"].as_u64().unwrap();
                    bridge.pending.lock().unwrap().remove(&id).unwrap().send(json!({ "result": {} })).unwrap();
                    request
                })
            });
            for session in sessions {
                let mut message = json!({ "id": 1, "method": "tools/call", "params": { "name": "tasks__list", "arguments": {} } });
                if let Some(id) = session {
                    message["params"]["_meta"] = json!({ "ai.opencode/sessionID": id });
                }
                handle(&bridge, message);
            }
            for (request, session) in received.join().unwrap().iter().zip(sessions) {
                assert_eq!(request["sessionId"], json!(session));
                assert_eq!(request["arguments"], json!({}));
                assert_eq!(request["ppid"], std::os::unix::process::parent_id());
            }
        });
    }

    /// The app's side for the round trip: one tool that says what it was called with.
    struct Echo;

    impl crate::server::Handler for Echo {
        fn tools(&self) -> Vec<Value> {
            vec![json!({ "name": "echo", "inputSchema": { "type": "object" } }), json!({ "name": "quiet", "cliOnly": true, "inputSchema": { "type": "object" } })]
        }
        fn instructions(&self) -> Vec<String> {
            vec!["Echo (echo): says it back.".into()]
        }
        fn call(&self, name: &str, arguments: Value, _: Option<u32>, _: Option<&str>) -> Result<crate::server::ToolResult, String> {
            Ok(crate::server::ToolResult { text: format!("{name} {arguments}"), is_error: false, images: Vec::new() })
        }
        fn hook(&self, _: &Value, _: Option<u32>) -> Value {
            json!({})
        }
        fn control(&self, _: &str, _: &Value) -> Option<Result<Value, String>> {
            None
        }
    }

    /// What the bridge writes to Claude Code.
    #[derive(Clone, Default)]
    struct Out(Arc<Mutex<Vec<u8>>>);

    impl Write for Out {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_tool_call_goes_through_the_socket_to_the_app_and_back() {
        let dir = std::env::temp_dir().join(format!("wings-bridge-{}", std::process::id()));
        let socket = dir.join("mcp.sock");
        crate::server::start(&socket, Arc::new(Echo), Arc::default()).unwrap();
        let out = Out::default();
        let bridge = Arc::new(Bridge {
            stdout: Mutex::new(Box::new(out.clone())),
            app: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            initialized: AtomicBool::new(false),
            version: "1.2.3",
            waits: Mutex::new(HashMap::new()),
        });
        link(&bridge, socket);
        let replies = |count: usize| -> Vec<Value> {
            let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
            assert_eq!(text.lines().count(), count, "{text}");
            text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        };
        handle(&bridge, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }));
        let init = &replies(1)[0]["result"];
        assert_eq!((init["protocolVersion"].as_str(), init["serverInfo"]["version"].as_str()), (Some("2025-06-18"), Some("1.2.3")));
        assert!(init["instructions"].as_str().unwrap().ends_with("\nEcho (echo): says it back."));
        handle(&bridge, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
        // The cli-only tool stays out of Claude's list, but the app still lists it for `wings tools` and a call still runs it.
        assert_eq!(replies(2)[1]["result"]["tools"], json!([{ "name": "echo", "inputSchema": { "type": "object" } }]));
        assert_eq!(bridge.ask(json!({ "op": "list" }), Duration::from_secs(5)).unwrap()["tools"][1]["name"], "quiet");
        handle(&bridge, json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": "quiet", "arguments": {} } }));
        assert_eq!(replies(3)[2]["result"]["content"][0]["text"], "quiet {}");
        handle(&bridge, json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "echo", "arguments": { "a": 1 } } }));
        assert_eq!(replies(4)[3], json!({ "jsonrpc": "2.0", "id": 3, "result": { "content": [{ "type": "text", "text": "echo {\"a\":1}" }], "isError": false } }));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tool that waits on the person (the Browser's demonstrate, up to an hour) is waited for as long as Wings gives it,
    /// where every call waited 310 s and then said Wings wasn't running; Claude Code isn't told of `maxSeconds`.
    #[test]
    fn a_tool_with_max_seconds_is_waited_for_that_long_and_claude_isnt_told_the_field() {
        let tools = Some(json!([
            { "name": "browser__demonstrate", "description": "d", "maxSeconds": 3615 },
            { "name": "tasks__list", "description": "l" },
            { "name": "browser__quick", "description": "q", "maxSeconds": 30 },
        ]));
        let waits = waits(&tools);
        assert_eq!(call_wait(&waits, Some("browser__demonstrate")), Duration::from_secs(3625));
        assert_eq!(call_wait(&waits, Some("tasks__list")), TOOL_WAIT);
        assert_eq!(call_wait(&waits, Some("browser__quick")), TOOL_WAIT, "never shorter than every tool gets");
        assert_eq!(call_wait(&waits, Some("setup_worktree")), SETUP_WORKTREE_WAIT);
        assert!(listed(tools).as_array().unwrap().iter().all(|tool| tool.get("maxSeconds").is_none()));
    }

    #[test]
    fn the_static_instructions_point_at_the_cli_for_what_the_tool_list_leaves_out() {
        assert!(INSTRUCTIONS.contains("`wings tools`") && INSTRUCTIONS.contains("wings call create_worktree"));
        // It was 2,941 characters before the worktree and plugin commands moved to Bash.
        assert!(INSTRUCTIONS.chars().count() < 2_941, "{}", INSTRUCTIONS.chars().count());
        for gone in ["setup_worktree", "worktree_setup", "disk__", "changes__"] {
            assert!(!INSTRUCTIONS.contains(gone), "{gone}");
        }
    }

    #[test]
    fn plugin_lines_follow_the_static_text() {
        assert_eq!(instructions(&[]), INSTRUCTIONS);
        let text = instructions(&["Issues (tasks__*): file issues.", "  ", "Timers (cyclops__*): track time."]);
        assert_eq!(text, format!("{INSTRUCTIONS}\nIssues (tasks__*): file issues.\nTimers (cyclops__*): track time."));
    }

    #[test]
    fn instructions_stay_under_the_cap_and_drop_whole_lines() {
        let long = "é".repeat(500);
        let lines = vec![long.as_str(); 12];
        let text = instructions(&lines);
        assert!(text.chars().count() <= MAX_INSTRUCTIONS);
        assert!(text.ends_with(&long));
        assert!(text.matches(&long).count() < 12);
    }
}
