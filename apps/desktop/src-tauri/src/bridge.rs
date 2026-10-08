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
find and read past Claude Code sessions on this computer. Tools from the plugins turned on in Wings are named \
<plugin>__<tool>, like cyclops__start_timer. With Wings closed there are none.";

struct Bridge {
    stdout: Mutex<std::io::Stdout>,
    app: Mutex<Option<UnixStream>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    next: AtomicU64,
    initialized: AtomicBool,
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

pub fn run(socket: Option<String>) -> i32 {
    let Some(socket) = socket.map(PathBuf::from) else {
        eprintln!("usage: wings --mcp <socket>");
        return 2;
    };
    let bridge = Arc::new(Bridge {
        stdout: Mutex::new(std::io::stdout()),
        app: Mutex::new(None),
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        initialized: AtomicBool::new(false),
    });

    // Keeps a connection to Wings, and reconnects after Wings quits or restarts.
    let link = bridge.clone();
    thread::spawn(move || loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
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

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let bridge = bridge.clone();
        // Each request on its own thread, so a slow tool call doesn't hold up the others.
        thread::spawn(move || handle(&bridge, message));
    }
    0
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
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": "wings", "title": "Wings", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools = bridge.ask(json!({ "op": "list" }), Duration::from_secs(5)).and_then(|r| r.get("tools").cloned());
            Ok(json!({ "tools": tools.unwrap_or_else(|| json!([])) }))
        }
        "tools/call" => {
            let request = json!({
                "op": "call",
                "name": message["params"]["name"],
                "arguments": message["params"].get("arguments").cloned().unwrap_or_else(|| json!({})),
                "ppid": std::os::unix::process::parent_id(),
            });
            Ok(match bridge.ask(request, Duration::from_secs(130)) {
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
