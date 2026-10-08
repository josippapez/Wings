//! `wings plugin ...`: install and manage plugins from a terminal or a script. It talks to the running Wings
//! over its socket, so the app's list, the Plugins sheet and Claude's tools stay in step.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
};

use serde_json::{json, Value};

/// `identifier` in tauri.conf.json. Tauri names the app's data folder after it.
const IDENTIFIER: &str = "dev.wings.app";

const USAGE: &str = "Usage: wings plugin <command>

  list [--json]           Installed plugins and whether they're on
  install <file | link>   Install a .wings-plugin file or a GitHub repo, like github.com/owner/name
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

/// One request to the running app.
fn ask(op: &str, mut request: Value) -> Result<Value, String> {
    let socket = dirs::data_dir().ok_or("Couldn't find the app data folder")?.join(IDENTIFIER).join("mcp.sock");
    let stream = UnixStream::connect(&socket).map_err(|_| "Wings isn't running. Open Wings and try again.".to_string())?;
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
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
    Err("Wings closed the connection".into())
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
    if p.get("dev").and_then(Value::as_bool).unwrap_or(false) {
        return "this repo".into();
    }
    match p["source"]["kind"].as_str() {
        Some("github") => format!("github.com/{}", p["source"]["repo"].as_str().unwrap_or_default()),
        _ => "file".into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn identifier_matches_the_tauri_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], super::IDENTIFIER);
    }
}
