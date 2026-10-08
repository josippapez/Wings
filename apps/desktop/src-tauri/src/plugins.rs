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
    /// Claude Code transcript entry types the plugin may read, e.g. `pr-link`.
    pub transcript: Vec<String>,
    /// URL prefixes the plugin may open in the browser.
    pub open_url: Vec<String>,
}

/// What a plugin adds to Wings, shown in the manager. The host refuses UI calls a plugin didn't declare.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contributes {
    /// UI it draws: `badges` on pane headers, the `diff` viewer.
    pub ui: Vec<String>,
    /// Tools it offers Claude through the Wings MCP server.
    pub mcp_tools: Vec<McpTool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
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
    let tool_name = |n: &str| !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if let Some(tool) = manifest.contributes.mcp_tools.iter().find(|t| !tool_name(&t.name)) {
        return Err(format!("MCP tool name {:?} must be lowercase letters, digits and _", tool.name));
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
fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let inherited = std::env::var("PATH").unwrap_or_default();
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let marker = "__WINGS_PATH__";
            let output = Command::new(shell)
                .args(["-l", "-i", "-c", &format!("printf '{marker}%s{marker}' \"$PATH\"")])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            if let Ok(out) = output {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(path) = text.split(marker).nth(1).filter(|p| !p.is_empty()) {
                    return format!("{path}:{inherited}");
                }
            }
        }
        inherited
    })
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

/// Entries of the allowed types from a Claude Code session transcript, oldest first.
pub fn transcript_entries(plugin: &Plugin, claude_dir: &Path, session_id: &str, types: &[String]) -> Result<Vec<Value>, String> {
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
    Ok(text
        .lines()
        .filter(|l| types.iter().any(|t| l.contains(&format!("\"type\":\"{t}\""))))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.get("type").and_then(Value::as_str).is_some_and(|t| types.iter().any(|x| x == t)))
        .collect())
}

pub fn may_open_url(plugin: &Plugin, url: &str) -> bool {
    url.starts_with("https://") && plugin.manifest.permissions.open_url.iter().any(|prefix| url.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

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
                },
                contributes: Contributes::default(),
            },
            dir: dir.to_path_buf(),
        }
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
        let entries = transcript_entries(&p, &claude, id, &["pr-link".into()]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["prNumber"], 7);
        assert!(transcript_entries(&p, &claude, id, &["user".into()]).is_err());
        assert!(transcript_entries(&p, &claude, "../../x", &["pr-link".into()]).is_err());
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn open_url_needs_https_and_a_declared_prefix() {
        let p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(may_open_url(&p, "https://github.com/a/b/pull/1"));
        assert!(!may_open_url(&p, "https://evil.example/"));
        assert!(!may_open_url(&p, "file:///etc/passwd"));
    }
}
