//! Third-party plugins: a folder with `wings-plugin.json` and web code that runs sandboxed in the UI.
//! Plugins never call Rust directly. The UI relays their requests, and everything that touches the
//! machine is checked here against the permissions the manifest declares.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, OnceLock},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Permissions {
    /// Commands the plugin may run: a program and the subcommand its arguments must start with, e.g.
    /// `gh pr view`. A bare program name allows any arguments.
    pub exec: Vec<String>,
    /// Claude Code transcript entry types the plugin may read, e.g. `pr-link`, or `attachment:<kind>` for one
    /// kind of attachment, e.g. `attachment:model`.
    pub transcript: Vec<String>,
    /// URL prefixes the plugin may open in the browser.
    pub open_url: Vec<String>,
    /// https URL prefixes the plugin may call with `wings.fetch`, like an API's base URL.
    pub fetch: Vec<String>,
    /// API paths the plugin may POST to through a signed-in CLI, as `<gh|glab|az> <pattern>`. `*` stands for
    /// one path segment, e.g. `gh repos/*/*/pulls/*/comments/*/replies`. `exec` refuses every write flag.
    pub post: Vec<String>,
    /// Lets the plugin open terminals in your projects and move focus to a pane. Each entry is a command it
    /// may start in a new pane, matched like `exec`; `[]` allows plain shells only. Absent, it can do neither.
    pub panes: Option<Vec<String>>,
    /// Lets the plugin show desktop notifications.
    pub notify: bool,
    /// Lets the plugin read what Claude Code last told `wings statusline`: usage limits and each session's
    /// context window and prompt cache.
    pub statusline: bool,
}

/// What a plugin adds to Wings, shown in the manager. The host refuses UI calls a plugin didn't declare.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contributes {
    /// UI it draws: `badges` on pane headers, the `diff` viewer.
    pub ui: Vec<String>,
    /// Tools it offers Claude through the Wings MCP server.
    pub mcp_tools: Vec<McpTool>,
    /// Web pages it shows in a popover from a title bar button.
    pub panels: Vec<Panel>,
    /// Its own pages shown in the right sidebar from a title bar button.
    pub sidebars: Vec<Sidebar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidebar {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`.
    pub icon: String,
    /// An HTML file in the plugin. It runs sandboxed like the plugin itself, with the same `wings` object.
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub title: String,
    /// One of `PANEL_ICONS`.
    pub icon: String,
    /// An https page. It runs as a normal website, with no access to Wings.
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

const PANEL_ICONS: [&str; 5] = ["clock", "globe", "calendar", "chart", "list"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's arguments; an object schema. Defaults to taking no arguments.
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    serde_json::json!({ "type": "object" })
}

const UI_KINDS: [&str; 2] = ["badges", "diff"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub api: u32,
    pub main: String,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

const API_VERSION: u32 = 1;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Loads every plugin folder under `roots`. Broken or incompatible plugins are skipped with a log line.
pub fn discover(roots: &[PathBuf]) -> HashMap<String, Plugin> {
    let mut found = HashMap::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            match load(&dir) {
                Ok(plugin) => {
                    found.entry(plugin.manifest.id.clone()).or_insert(plugin);
                }
                Err(e) if dir.join("wings-plugin.json").exists() => eprintln!("[plugins] skipped {}: {e}", dir.display()),
                Err(_) => {}
            }
        }
    }
    found
}

pub fn load(dir: &Path) -> Result<Plugin, String> {
    let text = fs::read_to_string(dir.join("wings-plugin.json")).map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if !valid_id(&manifest.id) {
        return Err(format!("invalid id {:?}", manifest.id));
    }
    if manifest.api != API_VERSION {
        return Err(format!("needs plugin API {}, this Wings has {API_VERSION}", manifest.api));
    }
    resolve(dir, &manifest.main).ok_or("main is outside the plugin folder")?;
    if let Some(kind) = manifest.contributes.ui.iter().find(|k| !UI_KINDS.contains(&k.as_str())) {
        return Err(format!("unknown ui {kind:?}, expected one of {UI_KINDS:?}"));
    }
    // Up to 60 so `<plugin id>__<tool>` stays within MCP's 128 characters.
    let tool_name = |n: &str| !n.is_empty() && n.len() <= 60 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if let Some(tool) = manifest.contributes.mcp_tools.iter().find(|t| !tool_name(&t.name)) {
        return Err(format!("MCP tool name {:?} must be up to 60 lowercase letters, digits and _", tool.name));
    }
    if let Some(tool) = manifest.contributes.mcp_tools.iter().find(|t| t.input_schema.get("type").and_then(Value::as_str) != Some("object")) {
        return Err(format!("MCP tool {:?} needs an inputSchema with \"type\": \"object\"", tool.name));
    }
    if manifest.contributes.panels.len() > 3 {
        return Err("at most 3 panels".into());
    }
    if manifest.contributes.sidebars.len() > 3 {
        return Err("at most 3 sidebars".into());
    }
    for sidebar in &manifest.contributes.sidebars {
        if !valid_id(&sidebar.id) || sidebar.title.is_empty() || sidebar.title.len() > 40 {
            return Err(format!("sidebar {:?} needs an id like a plugin id and a title up to 40 characters", sidebar.id));
        }
        if !PANEL_ICONS.contains(&sidebar.icon.as_str()) {
            return Err(format!("sidebar icon {:?} must be one of {PANEL_ICONS:?}", sidebar.icon));
        }
        resolve(dir, &sidebar.page).ok_or_else(|| format!("sidebar page {:?} isn't in the plugin folder", sidebar.page))?;
    }
    if let Some(prefix) = manifest.permissions.fetch.iter().find(|p| !fetch_prefix_ok(p)) {
        return Err(format!("fetch prefix {prefix:?} must be an https URL with a path, like https://api.example.com/"));
    }
    if let Some(entry) = manifest.permissions.post.iter().find(|e| post_entry(e).is_none()) {
        return Err(format!("post entry {entry:?} must be gh, glab or az and an API path, like gh repos/*/*/issues/*/comments"));
    }
    if let Some(entry) = manifest.permissions.panes.iter().flatten().find(|e| e.trim().is_empty() || !e.split_ascii_whitespace().all(plain_word)) {
        return Err(format!("panes entry {entry:?} must be a program and its arguments, using only letters, digits and -_./:=@%+,"));
    }
    for panel in &manifest.contributes.panels {
        if !valid_id(&panel.id) || panel.title.is_empty() || panel.title.len() > 40 {
            return Err(format!("panel {:?} needs an id like a plugin id and a title up to 40 characters", panel.id));
        }
        if !PANEL_ICONS.contains(&panel.icon.as_str()) {
            return Err(format!("panel icon {:?} must be one of {PANEL_ICONS:?}", panel.icon));
        }
        if !panel.url.starts_with("https://") || tauri::Url::parse(&panel.url).is_err() {
            return Err(format!("panel url {:?} must be an https URL", panel.url));
        }
    }
    Ok(Plugin { manifest, dir: dir.to_path_buf() })
}

/// A file inside the plugin folder; `None` if the path escapes it (`..`, symlinks out).
pub fn resolve(dir: &Path, relative: &str) -> Option<PathBuf> {
    let root = dir.canonicalize().ok()?;
    let path = root.join(relative.trim_start_matches('/')).canonicalize().ok()?;
    path.starts_with(&root).then_some(path)
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("html") => "text/html",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

const EXEC_TIMEOUT: Duration = Duration::from_secs(30);
/// Long enough for a browser sign-in (`az login`), short enough that nothing hangs forever.
const EXEC_TIMEOUT_MAX: Duration = Duration::from_secs(300);

pub fn exec_timeout(requested_ms: Option<u64>) -> Duration {
    requested_ms.map_or(EXEC_TIMEOUT, |ms| Duration::from_millis(ms).clamp(Duration::from_secs(1), EXEC_TIMEOUT_MAX))
}
const EXEC_OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;

/// Gets each output line while the program runs, for prompts like a sign-in code.
pub type OnLine = Arc<dyn Fn(&str) + Send + Sync>;

/// Reads a pipe on its own thread so a chatty program can't block on a full pipe.
fn read_pipe(pipe: impl Read + Send + 'static, on_line: Option<OnLine>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe.take(EXEC_OUTPUT_LIMIT));
        let (mut all, mut line) = (Vec::new(), Vec::new());
        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            if let Some(f) = &on_line {
                f(String::from_utf8_lossy(&line).trim_end());
            }
            all.append(&mut line);
        }
        String::from_utf8_lossy(&all).into_owned()
    })
}

/// Whether a `permissions.exec` entry covers this call: `gh pr view` allows `gh pr view <url> --json ...`.
fn may_exec(plugin: &Plugin, program: &str, args: &[String]) -> bool {
    plugin.manifest.permissions.exec.iter().any(|entry| {
        let mut words = entry.split_whitespace();
        words.next() == Some(program) && words.enumerate().all(|(i, word)| args.get(i).is_some_and(|a| a == word))
    })
}

/// Flags that turn an allowed command into running other commands, writing or reading files outside the
/// repo, changing data on the server, or printing a token. Refused whatever the manifest says.
fn is_unsafe_flag(program: &str, arg: &str) -> bool {
    // `abbrev`: git and az accept any unambiguous prefix of a long option, so `--upload=` means
    // `--upload-pack=`. gh and glab only take exact names.
    let (short, long, abbrev): (&[char], &[&str], bool) = match program {
        // Config overrides can point core.fsmonitor, core.sshCommand or an alias at any command.
        "git" => (
            &['c'],
            &["--config-env", "--exec-path", "--upload-pack", "--receive-pack", "--output", "--ext-diff", "--textconv", "--no-index"],
            true,
        ),
        "gh" | "glab" => (&['X', 'f', 'F', 't'], &["--method", "--field", "--raw-field", "--input", "--show-token"], false),
        // Short forms too: -f --file, -d --destination, -s --source, -m --method.
        "az" => (&['m', 'f', 'd', 's'], &["--http-method", "--method", "--in-file", "--out-file", "--output-file", "--file", "--destination", "--source"], true),
        _ => return false,
    };
    // az replaces `@path`, and the value in `key=@path` or `--flag=@path`, with that file's contents.
    if program == "az" && (arg.starts_with('@') || arg.split_once('=').is_some_and(|(_, value)| value.starts_with('@'))) {
        return true;
    }
    let name = arg.split('=').next().unwrap_or(arg);
    if let Some(option) = name.strip_prefix("--") {
        // An exact option name wins over abbreviations, so az's own `--output` isn't `--output-file`.
        let exact = program == "az" && name == "--output";
        return !option.is_empty() && !exact && long.iter().any(|flag| if abbrev { flag.starts_with(name) } else { *flag == name });
    }
    // Short flags group and carry their value attached, like `-iXDELETE` or `-ccore.fsmonitor=...`.
    name.strip_prefix('-').is_some_and(|group| group.chars().any(|c| short.contains(&c)))
}

/// Runs a command the manifest allows. No shell, so arguments can't smuggle in commands.
pub fn exec(
    plugin: &Plugin,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    on_line: Option<OnLine>,
) -> Result<ExecResult, String> {
    let id = &plugin.manifest.id;
    if !may_exec(plugin, program, args) {
        let sub = args.iter().take_while(|a| !a.starts_with('-')).take(3).cloned().collect::<Vec<_>>().join(" ");
        return Err(format!("{id} may not run {program} {sub}"));
    }
    if let Some(flag) = args.iter().find(|a| is_unsafe_flag(program, a)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    run(program, args, cwd, timeout, on_line)
}

/// Runs a program once its arguments have been checked.
fn run(program: &str, args: &[String], cwd: Option<&Path>, timeout: Duration, on_line: Option<OnLine>) -> Result<ExecResult, String> {
    let path = find_program(program).ok_or_else(|| format!("{program} is not installed or not on PATH"))?;
    let mut cmd = Command::new(path);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if program == "git" {
        // Makes git reject abbreviated options outright, on top of the prefix check above.
        cmd.env("GIT_TEST_DISALLOW_ABBREVIATED_OPTIONS", "1");
    }
    if let Some(dir) = cwd {
        if !dir.is_dir() {
            return Err(format!("{} is not a folder", dir.display()));
        }
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let out_reader = read_pipe(child.stdout.take().unwrap(), on_line.clone());
    let err_reader = read_pipe(child.stderr.take().unwrap(), on_line);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(format!("{program} timed out after {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(ExecResult { code: status.code(), stdout: out_reader.join().unwrap_or_default(), stderr: err_reader.join().unwrap_or_default() })
}

/// GUI apps don't get the shell's PATH on macOS and Linux, so ask the login shell once.
pub(crate) fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let inherited = std::env::var("PATH").unwrap_or_default();
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            if let Some(path) = login_shell_path(&shell, Duration::from_secs(5)) {
                return format!("{path}:{inherited}");
            }
        }
        inherited
    })
}

/// The PATH a login shell sets up, or `None` when that takes longer than `timeout`. A profile that runs
/// something long would otherwise hold up every program lookup. A job the profile starts in the background
/// can keep the output open after the shell exits, so it's read until the closing marker, not to the end.
#[cfg(unix)]
fn login_shell_path(shell: &str, timeout: Duration) -> Option<String> {
    const MARKER: &str = "__WINGS_PATH__";
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut text, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = stdout.read(&mut chunk) {
            text.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&text);
            let mut parts = text.split(MARKER);
            if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
                let _ = tx.send(path.to_string());
                return;
            }
        }
    });
    let path = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    path.filter(|p| !p.is_empty())
}

pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        return None;
    }
    let names: Vec<String> = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd")] } else { vec![program.into()] };
    std::env::split_paths(search_path())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// Whether a transcript entry is of a `permissions.transcript` type. `attachment:<kind>` is one kind of
/// attachment: attachments include whole files and hook output, so most plugins want only one kind.
fn is_type(allowed: &str, entry: &Value) -> bool {
    let kind = entry.get("type").and_then(Value::as_str);
    match allowed.strip_prefix("attachment:") {
        Some(sub) => kind == Some("attachment") && entry.pointer("/attachment/type").and_then(Value::as_str) == Some(sub),
        None => kind == Some(allowed),
    }
}

/// Text every line of that type holds, so the others are skipped before they're parsed.
fn type_marker(allowed: &str) -> String {
    format!("\"type\":\"{}\"", allowed.strip_prefix("attachment:").unwrap_or(allowed))
}

/// Entries of the allowed types from a Claude Code session transcript, oldest first.
/// The most entries `last` can ask for.
pub const TRANSCRIPT_LAST_MAX: usize = 1000;

/// With `last`, only the newest that many entries, read from the end. A long session's transcript runs to tens
/// of MB, and most callers want only its latest state.
pub fn transcript_entries(plugin: &Plugin, claude_dir: &Path, session_id: &str, types: &[String], last: Option<usize>) -> Result<Vec<Value>, String> {
    let allowed = &plugin.manifest.permissions.transcript;
    if let Some(t) = types.iter().find(|t| !allowed.contains(t)) {
        return Err(format!("{} may not read {t:?} transcript entries", plugin.manifest.id));
    }
    if session_id.len() != 36 || !session_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("not a session id".into());
    }
    let file = format!("{session_id}.jsonl");
    let path = fs::read_dir(claude_dir.join("projects"))
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.is_file())
        .ok_or("no transcript for that session")?;
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let patterns: Vec<String> = types.iter().map(|t| type_marker(t)).collect();
    let mentions = |l: &&str| patterns.iter().any(|p| l.contains(p.as_str()));
    // A nested object can say `"type":"user"` too, so the entry's own type is checked once it's parsed.
    let entry = |l: &str| serde_json::from_str::<Value>(l).ok().filter(|v| types.iter().any(|t| is_type(t, v)));
    Ok(match last {
        Some(n) => {
            let mut newest: Vec<Value> = text.lines().rev().filter(mentions).filter_map(entry).take(n.min(TRANSCRIPT_LAST_MAX)).collect();
            newest.reverse();
            newest
        }
        None => text.lines().filter(mentions).filter_map(entry).collect(),
    })
}

/// A fetch prefix has to name a host and end its host part with `/`, so `https://api.example.com` can't
/// also match `https://api.example.com.evil.net`.
fn fetch_prefix_ok(prefix: &str) -> bool {
    prefix.strip_prefix("https://").is_some_and(|rest| rest.split_once('/').is_some_and(|(host, _)| !host.is_empty() && !host.contains('@')))
}

/// The URL to send, if the plugin may call it: https, no credentials, under a `permissions.fetch` prefix,
/// and already in normal form. The path may only use letters, digits and `-_.~/`, with no `;` or percent
/// escapes, so no server can read it as a different path (`..;/`, `..%2F`, double encoding) than the one
/// that was checked.
pub fn fetch_url(plugin: &Plugin, url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let normal = parsed.as_str();
    let plain_path = parsed.path().bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~/".contains(&b));
    let ok = parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.fragment().is_none()
        && normal == url
        && plain_path
        && plugin.manifest.permissions.fetch.iter().any(|prefix| normal.starts_with(prefix));
    ok.then(|| normal.to_string())
}

/// The only headers a plugin may set. Anything else could route a request, and the token on it, to a
/// different site or path behind the same server (`Host`, `X-Forwarded-*`, `X-HTTP-Host-Override`).
pub fn fetch_header_ok(name: &str) -> bool {
    ["accept", "accept-language", "content-type", "if-none-match", "if-modified-since", "cache-control"].contains(&name.to_ascii_lowercase().as_str())
}

/// `permissions.post` entry → (program, pattern), if it's well formed.
fn post_entry(entry: &str) -> Option<(&str, &str)> {
    let (program, pattern) = entry.split_once(' ')?;
    // `?` and `=` for a fixed query like `?api-version=7.1`. A `*` only matches a whole plain segment.
    let chars_ok = !pattern.is_empty() && pattern.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~*/:%?=".contains(&b));
    let shape_ok = match program {
        "gh" | "glab" => !pattern.starts_with('/') && !pattern.contains(':'),
        "az" => pattern.strip_prefix("https://").is_some_and(|rest| rest.split('/').next().is_some_and(|host| !host.is_empty() && !host.contains('*'))),
        _ => false,
    };
    (chars_ok && shape_ok).then_some((program, pattern))
}

/// A URL segment matched by `*`. Percent escapes stay, since GitLab project paths (`g%2Fp`) and Azure project
/// names (`My%20Project`) need them, but no decoding of it may step out of its place (`%2e%2e`, `%252e`, `%5c`).
fn post_segment_ok(segment: &str) -> bool {
    if segment.is_empty() || !segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b)) {
        return false;
    }
    let bytes = segment.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
            let Some(byte) = hex else { return false };
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8_lossy(&decoded);
    !decoded.contains(['%', '\\', '?', '#']) && !decoded.split('/').any(|part| part == "." || part == "..")
}

/// Whether a `permissions.post` entry covers a POST by `program` to `url`.
fn may_post(plugin: &Plugin, program: &str, url: &str) -> bool {
    let parts: Vec<&str> = url.split('/').collect();
    plugin.manifest.permissions.post.iter().filter_map(|e| post_entry(e)).any(|(p, pattern)| {
        let want: Vec<&str> = pattern.split('/').collect();
        p == program && want.len() == parts.len() && want.iter().zip(&parts).all(|(w, part)| if *w == "*" { post_segment_ok(part) } else { w == part })
    })
}

/// A POST a plugin asks for: its fields become the JSON body. `host` is the GitLab server for `glab`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRequest {
    pub program: String,
    pub url: String,
    #[serde(default)]
    pub host: Option<String>,
    pub fields: serde_json::Map<String, Value>,
}

/// The resource `az rest` gets a token for when it calls Azure DevOps.
const AZURE_DEVOPS_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
const POST_BODY_LIMIT: usize = 64 * 1024;

fn hostname_ok(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && !host.starts_with(['-', '.']) && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// The CLI arguments for a POST the manifest allows. Wings builds them itself, so the plugin never picks a flag.
fn post_args(plugin: &Plugin, request: &PostRequest) -> Result<Vec<String>, String> {
    let PostRequest { program, url, host, fields } = request;
    let id = &plugin.manifest.id;
    if !may_post(plugin, program, url) {
        return Err(format!("{id} may not post to {program} {url}"));
    }
    let key_ok = |k: &str| !k.is_empty() && k.len() <= 40 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if fields.is_empty() || fields.len() > 10 {
        return Err("a post needs 1 to 10 fields".into());
    }
    if let Some((key, _)) = fields.iter().find(|(k, v)| !key_ok(k) || !(v.is_string() || v.is_i64())) {
        return Err(format!("post field {key:?} must be a name of letters, digits and _ with a string or whole number value"));
    }
    let body = Value::Object(fields.clone()).to_string();
    if body.len() > POST_BODY_LIMIT {
        return Err("a post is limited to 64 KB".into());
    }
    let mut args: Vec<String> = vec!["api".into()];
    match program.as_str() {
        "gh" | "glab" => {
            if program == "glab" {
                let host = host.as_deref().filter(|h| hostname_ok(h)).ok_or("a glab post needs the GitLab host name")?;
                args.extend(["--hostname".into(), host.into()]);
            }
            args.extend(["--method".into(), "POST".into(), url.clone()]);
            // `-f` sends the text as it is: only `-F` reads `@file` or fills in placeholders, and a number can't start with either.
            for (key, value) in fields {
                match value.as_str() {
                    Some(text) => args.extend(["-f".into(), format!("{key}={text}")]),
                    None => args.extend(["-F".into(), format!("{key}={value}")]),
                }
            }
        }
        // A JSON object starts with `{`, so az never reads it as `@file`.
        "az" => args = ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", url, "--body", &body, "-o", "json"].map(String::from).to_vec(),
        _ => unreachable!("may_post only matches gh, glab and az"),
    }
    Ok(args)
}

/// Sends a POST the manifest allows, like a reply to a review comment, as you through the CLI's own sign-in.
pub fn post(plugin: &Plugin, request: &PostRequest) -> Result<ExecResult, String> {
    let args = post_args(plugin, request)?;
    run(&request.program, &args, None, EXEC_TIMEOUT, None)
}

pub fn may_open_url(plugin: &Plugin, url: &str) -> bool {
    url.starts_with("https://") && plugin.manifest.permissions.open_url.iter().any(|prefix| url.starts_with(prefix))
}

/// A word of a pane command. Pane commands are typed into your shell, so words can't hold spaces, quotes,
/// backslashes, `$`, globs or operators; single-quoted, such a word reads the same in sh, bash, zsh and fish.
fn plain_word(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b))
}

/// Whether the plugin may read what Claude Code told `wings statusline`.
pub fn may_read_statusline(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.statusline
}

/// Whether the plugin may open terminals and move focus between panes.
pub fn may_use_panes(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.panes.is_some()
}

const PANE_COMMAND_MAX: usize = 1024;

/// The line to type into a new pane's shell for `command`, or `None` for a plain shell. Like `exec`, an entry
/// in `permissions.panes` is a program and the words the command must start with. Words past the entry
/// come from the plugin, so they're single-quoted.
pub fn pane_input(plugin: &Plugin, command: Option<&str>) -> Result<Option<String>, String> {
    let id = &plugin.manifest.id;
    let Some(declared) = &plugin.manifest.permissions.panes else {
        return Err(format!("{id} may not open panes"));
    };
    let Some(command) = command else { return Ok(None) };
    // Split on spaces only, so a tab or newline is refused rather than read as a word break.
    let words: Vec<&str> = command.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() || command.len() > PANE_COMMAND_MAX || !words.iter().all(|w| plain_word(w)) {
        return Err(format!("A pane command is up to 1 KB of words using only letters, digits and -_./:=@%+, not {command:?}"));
    }
    let entry = declared
        .iter()
        .map(|e| e.split_ascii_whitespace().collect::<Vec<_>>())
        .filter(|e| words.starts_with(e))
        .max_by_key(Vec::len)
        .ok_or_else(|| format!("{id} may not start {}", words.iter().take(3).copied().collect::<Vec<_>>().join(" ")))?;
    let program = words[0];
    if let Some(flag) = words[1..].iter().find(|w| is_unsafe_flag(program, w)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    let extra = words[entry.len()..].iter().map(|w| format!("'{w}'"));
    Ok(Some(entry.iter().map(|w| w.to_string()).chain(extra).collect::<Vec<_>>().join(" ")))
}

/// The folder a plugin's pane may start in: inside one of your projects once `..` and symlinks are resolved,
/// so neither can lead out of it. Returns which project it belongs to and the resolved folder. The project
/// on screen (`current`) wins when it holds the folder, so a split stays in its tab's project; otherwise the
/// deepest project that holds it.
pub fn pane_cwd(cwd: &Path, projects: &[PathBuf], current: Option<usize>) -> Result<(usize, PathBuf), String> {
    let dir = cwd.canonicalize().ok().filter(|d| d.is_dir()).ok_or_else(|| format!("{} is not a folder", cwd.display()))?;
    let roots: Vec<Option<PathBuf>> = projects.iter().map(|p| p.canonicalize().ok()).collect();
    let holds = |i: &usize| roots[*i].as_ref().is_some_and(|root| dir.starts_with(root));
    let depth = |i: &usize| roots[*i].as_ref().map_or(0, |root| root.components().count());
    let project = current.filter(holds).or_else(|| (0..roots.len()).filter(holds).max_by_key(depth));
    project.map(|i| (i, dir)).ok_or_else(|| format!("{} isn't inside any of your Wings projects", cwd.display()))
}

/// Notifications a plugin may show in any minute: enough for a few things finishing together, too few to
/// flood Notification Center.
pub const NOTIFY_LIMIT: usize = 3;
const NOTIFY_WINDOW: Duration = Duration::from_secs(60);
const NOTIFY_TITLE_MAX: usize = 64;
const NOTIFY_BODY_MAX: usize = 256;

/// When each plugin showed its recent notifications, for the rate limit.
#[derive(Default)]
pub struct NotifyLog(HashMap<String, Vec<Instant>>);

impl NotifyLog {
    /// Counts a notification at `now` if the plugin is under the limit, else says how long until it can.
    pub fn allow(&mut self, plugin_id: &str, now: Instant) -> Result<(), Duration> {
        let sent = self.0.entry(plugin_id.to_string()).or_default();
        sent.retain(|t| now.duration_since(*t) < NOTIFY_WINDOW);
        if sent.len() >= NOTIFY_LIMIT {
            return Err(NOTIFY_WINDOW - now.duration_since(sent[0]));
        }
        sent.push(now);
        Ok(())
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max - 1).chain(['…']).collect()
}

/// The title and body to show, cut to length. The plugin's name leads the title, so a plugin can't pass
/// for Wings or Claude.
pub fn notification(plugin: &Plugin, title: &str, body: &str) -> Result<(String, String), String> {
    let id = &plugin.manifest.id;
    if !plugin.manifest.permissions.notify {
        return Err(format!("{id} may not show notifications"));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("A notification needs a title".into());
    }
    Ok((format!("{}: {}", plugin.manifest.name, clip(title, NOTIFY_TITLE_MAX)), clip(body.trim(), NOTIFY_BODY_MAX)))
}

/// A transcript entry Claude just wrote, for `wings.onTranscript`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub session_id: String,
    pub pane_id: String,
    pub entry: Value,
    /// The running plugins whose `permissions.transcript` has the entry's type. Only they get it.
    pub plugins: Vec<String>,
}

/// The event for a line a transcript appended, if it's a JSON entry of a type one of `running` may read.
pub fn transcript_event(running: &[Plugin], line: crate::tail::Line) -> Option<TranscriptEvent> {
    let text = std::str::from_utf8(&line.bytes).ok()?;
    // Most lines are large tool output of a type no plugin reads, so look before parsing.
    let types = || running.iter().flat_map(|p| &p.manifest.permissions.transcript);
    if !types().any(|t| text.contains(&type_marker(t))) {
        return None;
    }
    let entry: Value = serde_json::from_str(text).ok()?;
    let plugins: Vec<String> =
        running.iter().filter(|p| p.manifest.permissions.transcript.iter().any(|t| is_type(t, &entry))).map(|p| p.manifest.id.clone()).collect();
    (!plugins.is_empty()).then_some(TranscriptEvent { session_id: line.session_id, pane_id: line.pane_id, entry, plugins })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn fake_shell(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("wings-shell-{name}-{}", std::process::id()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn login_shell_path_gives_up_on_a_slow_profile() {
        let quick = fake_shell("quick", "printf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        assert_eq!(login_shell_path(quick.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        // A job left running in the background keeps the output open after the shell is done.
        let background = fake_shell("background", "sleep 30 &\nprintf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        let start = Instant::now();
        assert_eq!(login_shell_path(background.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        let hung = fake_shell("hung", "sleep 30");
        let start = Instant::now();
        assert_eq!(login_shell_path(hung.to_str().unwrap(), Duration::from_millis(300)), None);
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        for path in [quick, background, hung] {
            let _ = fs::remove_file(path);
        }
    }

    fn plugin(dir: &Path, exec: &[&str], transcript: &[&str]) -> Plugin {
        Plugin {
            manifest: Manifest {
                id: "t".into(),
                name: "T".into(),
                version: "0".into(),
                description: String::new(),
                api: 1,
                main: "main.js".into(),
                permissions: Permissions {
                    exec: exec.iter().map(|s| s.to_string()).collect(),
                    transcript: transcript.iter().map(|s| s.to_string()).collect(),
                    open_url: vec!["https://github.com/".into()],
                    fetch: vec!["https://api.example.com/v1/".into()],
                    ..Default::default()
                },
                contributes: Contributes::default(),
            },
            dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn fetch_only_reaches_declared_https_prefixes() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert_eq!(fetch_url(&p, "https://api.example.com/v1/timers").as_deref(), Some("https://api.example.com/v1/timers"));
        for url in [
            "http://api.example.com/v1/timers",
            "https://api.example.com/v2/x",
            "https://api.example.com.evil.net/v1/",
            "https://user:pw@api.example.com/v1/",
            "not a url",
            // Paths a server might decode or normalise to somewhere outside /v1/.
            "https://api.example.com/v1/../admin",
            "https://api.example.com/v1/..%2Fadmin",
            "https://api.example.com/v1/%2e%2e/admin",
            "https://api.example.com/v1\\..\\admin",
            "https://API.example.com/v1/x",
            "https://api.example.com/v1/..;/admin",
            "https://api.example.com/v1/%252e%252e%252fadmin",
            "https://api.example.com/v1/a%20b",
            "https://api.example.com/v1/x#frag",
        ] {
            assert!(fetch_url(&p, url).is_none(), "{url}");
        }
        assert_eq!(fetch_url(&p, "https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08").as_deref(), Some("https://api.example.com/v1/time-entries?from=2026-10-05&to=2026-10-08"));
        for name in ["Accept", "content-type", "If-None-Match"] {
            assert!(fetch_header_ok(name), "{name}");
        }
        for name in [
            "Host", "x-forwarded-host", "X-Forwarded", "Forwarded", "Cookie", "Transfer-Encoding", "Authorization", "X-Real-IP",
            "X-HTTP-Host-Override", "X-HTTP-Method-Override", "X-Override-URL", "X-Api-Key",
        ] {
            assert!(!fetch_header_ok(name), "{name}");
        }
        assert!(fetch_prefix_ok("https://api.example.com/"));
        assert!(!fetch_prefix_ok("https://api.example.com") && !fetch_prefix_ok("http://x.com/") && !fetch_prefix_ok("https:///x"));
    }

    #[test]
    fn exec_needs_the_declared_subcommand() {
        let p = plugin(Path::new("/tmp"), &["echo hi", "git remote get-url"], &[]);
        assert_eq!(exec(&p, "echo", &["hi".into(), "there".into()], None, EXEC_TIMEOUT, None).unwrap().stdout, "hi there\n");
        assert!(exec(&p, "echo", &["bye".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run echo bye"));
        assert!(exec(&p, "git", &["status".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run git status"));
        let missing = Path::new("/no/such/folder");
        assert!(exec(&p, "echo", &["hi".into()], Some(missing), EXEC_TIMEOUT, None).unwrap_err().contains("is not a folder"));
    }

    #[test]
    fn flags_that_run_code_or_write_are_refused() {
        for (program, arg) in [
            ("git", "-c"),
            ("git", "-ccore.fsmonitor=touch x"),
            ("git", "--upload-pack=sh"),
            ("git", "--output=/tmp/x"),
            ("git", "--no-index"),
            ("git", "--upload=sh"),
            ("git", "--outp=/tmp/x"),
            ("gh", "-XDELETE"),
            ("gh", "-iXDELETE"),
            ("glab", "-pfquery=mutation"),
            ("az", "--http-meth=POST"),
            ("az", "--out-file=/tmp/x"),
            ("az", "--out-f"),
            ("az", "@/etc/passwd"),
            ("az", "--body=@/etc/passwd"),
            ("az", "project=@/etc/passwd"),
            ("az", "--output-file=/tmp/x"),
            ("az", "-mPOST"),
            ("az", "-f/etc/passwd"),
            ("az", "-d"),
            ("az", "--source"),
            ("gh", "--method=POST"),
            ("gh", "-fquery=mutation"),
            ("glab", "--show-token"),
            ("glab", "-t"),
            ("az", "--http-method"),
        ] {
            assert!(is_unsafe_flag(program, arg), "{program} {arg}");
        }
        for (program, arg) in [
            ("git", "-C"),
            ("git", "--cached"),
            ("git", "--quiet"),
            ("git", "--no-ext-diff"),
            ("git", "--no-color"),
            ("gh", "--json"),
            ("gh", "-i"),
            ("glab", "--paginate"),
            ("glab", "--raw"),
            ("az", "-o"),
            ("az", "--output"),
            ("az", "--source-branch"),
        ] {
            assert!(!is_unsafe_flag(program, arg), "{program} {arg}");
        }
        // The escape this guards against: a config override that runs a shell command.
        let p = plugin(Path::new("/tmp"), &["git"], &[]);
        let args = ["-c".into(), "core.fsmonitor=touch /tmp/wings-pwned".into(), "status".into()];
        assert!(exec(&p, "git", &args, None, EXEC_TIMEOUT, None).unwrap_err().contains("may not pass -c"));
    }

    #[test]
    fn pr_tracker_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/pr-tracker"));
        let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("wings-plugin.json")).unwrap()).unwrap();
        let p = Plugin { manifest, dir: dir.to_path_buf() };
        let calls: &[(&str, &[&str])] = &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["branch", "--show-current"]),
            ("git", &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]),
            ("git", &["fetch", "--quiet", "origin", "main", "feature/x"]),
            ("git", &["diff", "--no-color", "--no-ext-diff", "origin/main...origin/feature/x"]),
            ("gh", &["pr", "list", "--head", "x", "--state", "all", "--limit", "1", "--json", "url,number"]),
            ("gh", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "comments,reviews"]),
            ("gh", &["pr", "diff", "https://github.com/o/r/pull/1"]),
            ("gh", &["api", "repos/o/r/pulls/1/comments", "--paginate", "--slurp"]),
            ("gh", &["auth", "login", "--web", "--clipboard", "--hostname", "github.com"]),
            ("glab", &["api", "--hostname", "gitlab.com", "--paginate", "projects/g%2Fp/merge_requests/1/discussions?per_page=100"]),
            ("glab", &["mr", "diff", "1", "--raw", "--repo", "https://gitlab.com/g/p"]),
            ("glab", &["auth", "status", "--hostname", "gitlab.com"]),
            ("glab", &["auth", "login", "--web", "--hostname", "gitlab.com"]),
            ("az", &["repos", "pr", "list", "--organization", "https://dev.azure.com/o", "--source-branch", "x", "-o", "json"]),
            ("az", &["repos", "pr", "policy", "list", "--id", "1", "-o", "json"]),
            ("az", &["devops", "invoke", "--area", "git", "--resource", "pullRequestThreads", "--api-version", "7.1"]),
            ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
        ];
        for (program, args) in calls {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(may_exec(&p, program, &args), "{program} {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag(program, a)), "{program} {args:?} refused");
        }
        assert!(!may_exec(&p, "gh", &["auth".into(), "token".into()]));
        let posts = [
            ("gh", "repos/o/r/pulls/1/comments/2/replies", None),
            ("glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes", Some("gitlab.com")),
            ("az", "https://dev.azure.com/o/My%20Project/_apis/git/repositories/Repo.Name/pullRequests/1/threads/44590/comments?api-version=7.1", None),
        ];
        for (program, url, host) in posts {
            assert!(post_args(&p, &post_request(program, url, host)).is_ok(), "{program} {url} not declared");
        }
    }

    #[test]
    fn work_item_calls_pass_its_own_manifest() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins/work-item"));
        let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("wings-plugin.json")).unwrap()).unwrap();
        let p = Plugin { manifest, dir: dir.to_path_buf() };
        let org = "https://dev.azure.com/o";
        let calls: &[(&str, &[&str])] = &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["branch", "--show-current"]),
            ("az", &["boards", "work-item", "show", "--id", "123456", "--organization", org, "-o", "json"]),
            (
                "az",
                &[
                    "devops", "invoke", "--area", "wit", "--resource", "workItemTypeStates", "--route-parameters",
                    "project=My Project", "type=Tech Story", "--api-version", "7.1", "--organization", org, "-o", "json",
                ],
            ),
            ("az", &["boards", "work-item", "update", "--id", "123456", "--state", "Ready for Peer Review/QA", "--organization", org, "-o", "json"]),
            ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
        ];
        for (program, args) in calls {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert!(may_exec(&p, program, &args), "{program} {args:?} not declared");
            assert!(!args.iter().any(|a| is_unsafe_flag(program, a)), "{program} {args:?} refused");
        }
        // The update entry can't read a file into a field or the state, and can't reach other commands.
        let update = |extra: &[&str]| -> Vec<String> { ["boards", "work-item", "update", "--id", "1"].iter().chain(extra).map(|a| a.to_string()).collect() };
        for extra in [&["--state", "@/etc/passwd"][..], &["--fields", "System.Title=@/etc/passwd"], &["--description=@notes.md"], &["-f", "x=1"], &["-d", "x"]] {
            assert!(update(extra).iter().any(|a| is_unsafe_flag("az", a)), "{extra:?}");
        }
        assert!(!may_exec(&p, "az", &["boards".into(), "work-item".into(), "delete".into(), "--id".into(), "1".into()]));
        assert!(!may_exec(&p, "az", &["rest".into()]));
        assert!(p.manifest.permissions.post.is_empty());
    }

    fn post_request(program: &str, url: &str, host: Option<&str>) -> PostRequest {
        let fields = serde_json::json!({ "body": "Thanks, fixed" }).as_object().unwrap().clone();
        PostRequest { program: program.into(), url: url.into(), host: host.map(String::from), fields }
    }

    #[test]
    fn post_only_reaches_declared_paths() {
        let mut p = plugin(Path::new("/tmp"), &["gh api"], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/merge_requests/*/discussions/*/notes".into()];
        assert!(may_post(&p, "gh", "repos/o/r/pulls/1/comments/2/replies"));
        for url in [
            "repos/o/r/pulls/1/comments/2/replies/x",
            "repos/o/r/pulls/1/comments/replies",
            "repos/o/r/pulls/1/comments/../replies",
            "repos/o/r/pulls/1/comments/%2e%2e/replies",
            "repos/o/r/pulls/1/comments/%252e/replies",
            "repos/o/r/pulls/1/comments/%5c/replies",
            "repos/o/r/pulls/1/comments/2?x=1/replies",
            "repos/o/r/pulls/1/comments/a%2F..%2Fb/replies",
        ] {
            assert!(!may_post(&p, "gh", url), "{url}");
        }
        assert!(!may_post(&p, "glab", "repos/o/r/pulls/1/comments/2/replies"));
        assert!(may_post(&p, "glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes"));
        // An exec permission for `gh api` still can't write.
        assert!(is_unsafe_flag("gh", "--method"));
    }

    #[test]
    fn post_builds_the_command_itself() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.post = vec!["gh repos/*/*/pulls/*/comments/*/replies".into(), "glab projects/*/notes".into(), "az https://dev.azure.com/*/threads".into()];
        let args = post_args(&p, &post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None)).unwrap();
        assert_eq!(args, ["api", "--method", "POST", "repos/o/r/pulls/1/comments/2/replies", "-f", "body=Thanks, fixed"]);
        let mut at_file = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        at_file.fields.insert("body".into(), "@/etc/passwd".into());
        assert_eq!(post_args(&p, &at_file).unwrap()[5], "body=@/etc/passwd", "sent as text with -f");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", None)).is_err(), "glab needs a host");
        assert!(post_args(&p, &post_request("glab", "projects/1/notes", Some("-x"))).is_err());
        let az = post_args(&p, &post_request("az", "https://dev.azure.com/o/threads", None)).unwrap();
        assert_eq!(az[..7], ["rest", "--method", "post", "--resource", AZURE_DEVOPS_RESOURCE, "--url", "https://dev.azure.com/o/threads"]);
        assert_eq!(az[8], r#"{"body":"Thanks, fixed"}"#);
        let mut bad = post_request("gh", "repos/o/r/pulls/1/comments/2/replies", None);
        bad.fields.insert("--x".into(), "1".into());
        assert!(post_args(&p, &bad).is_err());
        bad.fields = serde_json::Map::new();
        assert!(post_args(&p, &bad).is_err());
    }

    #[test]
    fn post_entries_are_checked_on_load() {
        for entry in ["gh repos/*/replies", "glab projects/*/notes", "az https://dev.azure.com/*/comments"] {
            assert!(post_entry(entry).is_some(), "{entry}");
        }
        for entry in ["curl https://x/", "gh /repos/x", "gh https://api.github.com/x", "az dev.azure.com/x", "az https://*/x", "gh repos/x y", "gh "] {
            assert!(post_entry(entry).is_none(), "{entry}");
        }
    }

    #[test]
    fn exec_only_runs_declared_programs_without_a_shell() {
        let p = plugin(Path::new("/tmp"), &["echo"], &[]);
        let out = exec(&p, "echo", &["hi; rm -rf /".into()], None, EXEC_TIMEOUT, None).unwrap();
        assert_eq!(out.stdout.trim(), "hi; rm -rf /");
        assert!(exec(&p, "ls", &[], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run"));
        assert!(exec(&p, "/bin/echo", &[], None, EXEC_TIMEOUT, None).is_err());
    }

    #[test]
    fn exec_streams_lines_from_both_pipes() {
        let p = plugin(Path::new("/tmp"), &["sh"], &[]);
        let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = lines.clone();
        let on_line: OnLine = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        let out = exec(&p, "sh", &["-c".into(), "echo out; echo err >&2".into()], None, EXEC_TIMEOUT, Some(on_line)).unwrap();
        let mut got = lines.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["err", "out"]);
        assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("out\n", "err\n"));
    }

    #[test]
    fn exec_timeout_is_capped() {
        assert_eq!(exec_timeout(None), EXEC_TIMEOUT);
        assert_eq!(exec_timeout(Some(10)), Duration::from_secs(1));
        assert_eq!(exec_timeout(Some(3_600_000)), EXEC_TIMEOUT_MAX);
        let p = plugin(Path::new("/tmp"), &["sleep"], &[]);
        assert!(exec(&p, "sleep", &["5".into()], None, Duration::from_secs(1), None).unwrap_err().contains("timed out"));
    }

    #[test]
    fn files_cannot_escape_the_plugin_folder() {
        let dir = std::env::temp_dir().join(format!("wings-plugin-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        assert!(resolve(&dir, "/main.js").is_some());
        assert!(resolve(&dir, "../../etc/passwd").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcript_reads_only_declared_types() {
        let claude = std::env::temp_dir().join(format!("wings-tr-{}", std::process::id()));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        fs::write(
            claude.join(format!("projects/-x/{id}.jsonl")),
            "{\"type\":\"user\",\"message\":{}}\n{\"type\":\"pr-link\",\"prNumber\":7,\"prUrl\":\"https://github.com/a/b/pull/7\"}\n",
        )
        .unwrap();
        let p = plugin(&claude, &[], &["pr-link"]);
        let entries = transcript_entries(&p, &claude, id, &["pr-link".into()], None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["prNumber"], 7);
        assert!(transcript_entries(&p, &claude, id, &["user".into()], None).is_err());
        assert!(transcript_entries(&p, &claude, "../../x", &["pr-link".into()], None).is_err());
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_last_gives_the_newest_entries_in_order() {
        let claude = std::env::temp_dir().join(format!("wings-tr-last-{}", std::process::id()));
        let id = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        // The assistant entry mentions `"type":"user"` inside its content, which mustn't make it a user entry.
        let lines = [
            r#"{"type":"user","n":1}"#,
            r#"{"type":"assistant","n":2,"message":{"content":[{"type":"user"}]}}"#,
            r#"{"type":"user","n":3}"#,
            "not json",
            r#"{"type":"user","n":4}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["user"]);
        let n = |entries: Vec<Value>| entries.iter().map(|e| e["n"].as_i64().unwrap()).collect::<Vec<_>>();
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], Some(2)).unwrap()), [3, 4]);
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], Some(10)).unwrap()), [1, 3, 4]);
        assert_eq!(n(transcript_entries(&p, &claude, id, &["user".into()], None).unwrap()), [1, 3, 4]);
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_events_go_only_to_plugins_that_read_the_type() {
        let reader = |id: &str, types: &[&str]| {
            let mut p = plugin(Path::new("/tmp"), &[], types);
            p.manifest.id = id.into();
            p
        };
        let running = [reader("timeline", &["user", "assistant"]), reader("cost", &["assistant", "cost-state"]), reader("pr", &["pr-link"])];
        let event = |text: &str| {
            let line = crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() };
            transcript_event(&running, line)
        };
        let e = event(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#).unwrap();
        assert_eq!((e.plugins.as_slice(), e.pane_id.as_str(), e.session_id.as_str()), (["timeline".to_string(), "cost".to_string()].as_slice(), "p1", "s1"));
        assert_eq!(e.entry["message"]["content"][0]["text"], "hi");
        assert_eq!(event(r#"{"type":"cost-state","costUSD":1.5}"#).unwrap().plugins, ["cost"]);
        assert_eq!(event(r#"{"type":"pr-link","prNumber":7}"#).unwrap().plugins, ["pr"]);
        // A type nobody reads, even when a nested block names a type someone does, and lines that aren't entries.
        assert!(event(r#"{"type":"attachment","content":[{"type":"user"}]}"#).is_none());
        for line in [r#"{"type":"user""#, "not json", r#"["type","user"]"#, r#"{"kind":"user","x":{"type":"user"}}"#, r#"{"type":7}"#] {
            assert!(event(line).is_none(), "{line}");
        }
        let line = |bytes: &[u8]| crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: bytes.to_vec() };
        assert!(transcript_event(&running, line(b"{\"type\":\"user\",\"x\":\"\xff\"}")).is_none());
        assert!(transcript_event(&[], line(b"{\"type\":\"user\"}")).is_none());
    }

    #[test]
    fn attachment_kinds_narrow_what_a_plugin_reads() {
        let claude = std::env::temp_dir().join(format!("wings-tr-att-{}", std::process::id()));
        let id = "5c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        let lines = [
            r#"{"type":"attachment","attachment":{"type":"model","identity":{"modelId":"claude-opus-5-5[1m]"}}}"#,
            r#"{"type":"attachment","attachment":{"type":"file","content":"secret source"}}"#,
            // Says `"type":"model"` inside, but isn't a model attachment.
            r#"{"type":"user","message":{"content":[{"type":"model"}]}}"#,
            r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","failedMcpServers":[]}}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["attachment:model", "attachment:deferred_tools_delta"]);
        let kinds = |entries: Vec<Value>| entries.iter().map(|e| e["attachment"]["type"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let both = ["attachment:model".into(), "attachment:deferred_tools_delta".into()];
        assert_eq!(kinds(transcript_entries(&p, &claude, id, &both, None).unwrap()), ["model", "deferred_tools_delta"]);
        assert_eq!(kinds(transcript_entries(&p, &claude, id, &["attachment:model".into()], Some(5)).unwrap()), ["model"]);
        // One kind doesn't allow every attachment, or another kind.
        assert!(transcript_entries(&p, &claude, id, &["attachment".into()], None).is_err());
        assert!(transcript_entries(&p, &claude, id, &["attachment:file".into()], None).is_err());
        // A plain `attachment` permission still reads them all.
        let all = plugin(&claude, &[], &["attachment"]);
        assert_eq!(transcript_entries(&all, &claude, id, &["attachment".into()], None).unwrap().len(), 3);
        let _ = fs::remove_dir_all(&claude);

        let mut narrow = plugin(Path::new("/tmp"), &[], &["attachment:model"]);
        narrow.manifest.id = "narrow".into();
        let mut wide = plugin(Path::new("/tmp"), &[], &["attachment"]);
        wide.manifest.id = "wide".into();
        let running = [narrow, wide];
        let event = |text: &str| transcript_event(&running, crate::tail::Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() });
        assert_eq!(event(lines[0]).unwrap().plugins, ["narrow", "wide"]);
        assert_eq!(event(lines[1]).unwrap().plugins, ["wide"]);
        assert!(event(lines[2]).is_none());
    }

    #[test]
    fn statusline_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-statusline-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_statusline(&manifest(serde_json::json!({}))));
        assert!(!may_read_statusline(&manifest(serde_json::json!({ "transcript": ["assistant"] }))));
        assert!(may_read_statusline(&manifest(serde_json::json!({ "statusline": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_url_needs_https_and_a_declared_prefix() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(may_open_url(&p, "https://github.com/a/b/pull/1"));
        assert!(!may_open_url(&p, "https://evil.example/"));
        assert!(!may_open_url(&p, "file:///etc/passwd"));
    }

    fn with_panes(panes: Option<&[&str]>) -> Plugin {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.panes = panes.map(|list| list.iter().map(|s| s.to_string()).collect());
        p
    }

    #[test]
    fn pane_commands_must_be_declared() {
        let p = with_panes(Some(&["lazygit", "npm run dev", "claude --resume", "git log"]));
        let input = |command: Option<&str>| pane_input(&p, command);
        assert_eq!(input(None).unwrap(), None);
        assert_eq!(input(Some("lazygit")).unwrap().as_deref(), Some("lazygit"));
        assert_eq!(input(Some("  npm   run dev ")).unwrap().as_deref(), Some("npm run dev"));
        // Words the plugin adds past the declared entry are quoted.
        assert_eq!(input(Some("npm run dev --port=3000")).unwrap().as_deref(), Some("npm run dev '--port=3000'"));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        assert_eq!(input(Some(&format!("claude --resume {id}"))).unwrap(), Some(format!("claude --resume '{id}'")));

        for undeclared in ["rm -rf /", "npm run build", "npm", "claude", "lazygitx", "/usr/bin/lazygit"] {
            assert!(input(Some(undeclared)).unwrap_err().contains("may not start"), "{undeclared}");
        }
        // Nothing a shell would read as more than plain words: operators, quotes, expansions, globs, newlines.
        for smuggled in [
            "lazygit; rm -rf ~", "lazygit && curl x", "lazygit | sh", "lazygit $(id)", "lazygit `id`", "lazygit 'a'",
            "lazygit a\\", "lazygit ~", "lazygit *", "lazygit >x", "lazygit\nrm", "lazygit\r", "npm run dev #", "",
        ] {
            assert!(input(Some(smuggled)).unwrap_err().contains("only letters"), "{smuggled:?}");
        }
        assert!(input(Some(&format!("lazygit {}", "a".repeat(1100)))).is_err());
        // The same flags `exec` refuses.
        assert!(input(Some("git log -ccore.pager=sh")).unwrap_err().contains("may not pass"));

        // `[]` allows plain shells only; no `panes` allows nothing, not even focus.
        let shells = with_panes(Some(&[]));
        assert_eq!(pane_input(&shells, None).unwrap(), None);
        assert!(pane_input(&shells, Some("lazygit")).unwrap_err().contains("may not start"));
        assert!(may_use_panes(&shells));
        let none = with_panes(None);
        assert!(pane_input(&none, None).unwrap_err().contains("may not open panes"));
        assert!(!may_use_panes(&none));
    }

    #[test]
    fn pane_entries_are_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-panes-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |panes: &[&str]| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": { "panes": panes } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        assert_eq!(manifest(&["lazygit", "npm run dev"]).unwrap().manifest.permissions.panes.unwrap(), ["lazygit", "npm run dev"]);
        for bad in ["npm run dev && curl x | sh", "", " ", "echo $HOME"] {
            assert!(manifest(&[bad]).err().unwrap().contains("panes entry"), "{bad:?}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pane_cwd_must_be_inside_a_project() {
        // Canonical, since macOS temp folders sit behind the /var symlink.
        let root = std::env::temp_dir().join(format!("wings-cwd-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        for dir in ["app/src", "app/nested/deep", "app2", "outside"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("app/file.txt"), "").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("app/link-out")).unwrap();
        let projects = [root.join("app"), root.join("app/nested"), root.join("app2")];
        let cwd = |dir: &str, current| pane_cwd(&root.join(dir), &projects, current);

        assert_eq!(cwd("app", None).unwrap(), (0, root.join("app")));
        assert_eq!(cwd("app/src", None).unwrap(), (0, root.join("app/src")));
        // The deepest project holds it, unless the project on screen does.
        assert_eq!(cwd("app/nested/deep", None).unwrap().0, 1);
        assert_eq!(cwd("app/nested/deep", Some(0)).unwrap().0, 0);
        assert_eq!(cwd("app/src", Some(2)).unwrap().0, 0);

        for outside in ["outside", "app/../outside", "app/link-out", "app/src/../../outside", "."] {
            assert!(cwd(outside, Some(0)).unwrap_err().contains("isn't inside"), "{outside}");
        }
        // `app2` only shares a prefix with `app`; with app2 gone it's in no project.
        assert!(pane_cwd(&root.join("app2"), &projects[..2], None).unwrap_err().contains("isn't inside"));
        assert!(pane_cwd(Path::new("/"), &projects, None).unwrap_err().contains("isn't inside"));
        assert!(cwd("app/missing", None).unwrap_err().contains("is not a folder"));
        assert!(cwd("app/file.txt", None).unwrap_err().contains("is not a folder"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn notifications_need_permission_are_cut_to_length_and_rate_limited() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(notification(&p, "Done", "").unwrap_err().contains("may not show notifications"));
        p.manifest.permissions.notify = true;
        assert_eq!(notification(&p, " Build passed ", "main is green").unwrap(), ("T: Build passed".into(), "main is green".into()));
        assert!(notification(&p, "  ", "body").unwrap_err().contains("needs a title"));
        let (title, body) = notification(&p, &"t".repeat(500), &"ü".repeat(5000)).unwrap();
        assert_eq!((title.chars().count(), body.chars().count()), (3 + NOTIFY_TITLE_MAX, NOTIFY_BODY_MAX));
        assert!(body.ends_with('…'));

        let mut log = NotifyLog::default();
        let start = Instant::now();
        for i in 0..NOTIFY_LIMIT as u64 {
            assert!(log.allow("a", start + Duration::from_secs(i)).is_ok());
        }
        assert_eq!(log.allow("a", start + Duration::from_secs(10)), Err(Duration::from_secs(50)));
        // Each plugin has its own budget, and a refused call doesn't use any.
        assert!(log.allow("b", start + Duration::from_secs(10)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(59)).is_err());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_err());
    }
}
