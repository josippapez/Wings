//! The coding agent CLIs Wings knows: whether each is on this Mac, its version, and whether it's signed in, as the
//! desktop's `harnesses.rs`, so both apps check them the same way. Each check runs the CLI's own status command from
//! this fixed table, when Settings, Agents opens. The login and install commands the page types into a tab come from
//! here too.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

/// A CLI that hangs, or waits on the network, doesn't hold up the page for longer than this.
pub const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Auth {
    SignedIn,
    SignedOut,
    Unknown,
}

/// What a status command said: the sign-in state and, when it names one, the account.
type SignIn = (Auth, Option<String>);

struct Harness {
    id: &'static str,
    name: &'static str,
    /// The names it installs as, the first found wins.
    programs: &'static [&'static str],
    /// Its sign-in check, run with the program found. None when the CLI has no cheap, documented one.
    status: Option<&'static [&'static str]>,
    /// Stop at the first line of output: the check goes on to slow work after it says who you are.
    first_line: bool,
    env: &'static [(&'static str, &'static str)],
    read: fn(code: Option<i32>, out: &str) -> SignIn,
    /// Run after the program name.
    login: &'static str,
    install: &'static str,
    docs: &'static str,
}

/// Status commands and their output, from each CLI's docs or source (the desktop's table):
/// - Claude Code: `claude auth status` exits 0 signed in, 1 not, and prints JSON with `email` (code.claude.com/docs/en/cli-reference).
/// - OpenCode: `opencode auth list --format json` lists the providers with stored credentials (opencode.ai/docs/cli).
/// - Codex: `codex login status` exits 0 with `Logged in using …` or `API key configured`, 1 with `Not logged in` (codex-rs/cli/src/login.rs).
/// - Grok: `grok models` first prints `You are logged in with <host>.` or `You are not authenticated.`, then starts the agent
///   to list models (xai-org/grok-build, xai-grok-pager/src/models.rs).
/// - Cursor: `agent status` isn't documented beyond its purpose, so it isn't run.
const HARNESSES: [Harness; 5] = [
    Harness {
        id: "claude",
        name: "Claude Code",
        programs: &["claude"],
        status: Some(&["auth", "status"]),
        first_line: false,
        env: &[],
        read: read_claude,
        login: "auth login",
        install: "curl -fsSL https://claude.ai/install.sh | bash",
        docs: "https://code.claude.com/docs/en/setup",
    },
    Harness {
        id: "codex",
        name: "Codex",
        programs: &["codex"],
        status: Some(&["login", "status"]),
        first_line: false,
        env: &[],
        read: read_codex,
        login: "login",
        install: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        docs: "https://developers.openai.com/codex/cli",
    },
    Harness {
        id: "cursor",
        name: "Cursor CLI",
        // `agent` is the current name and `cursor-agent` the old one; the installer adds both.
        programs: &["cursor-agent", "agent"],
        status: None,
        first_line: false,
        env: &[],
        read: |_, _| (Auth::Unknown, None),
        login: "login",
        install: "curl https://cursor.com/install -fsS | bash",
        docs: "https://cursor.com/docs/cli/overview",
    },
    Harness {
        id: "grok",
        name: "Grok Build",
        programs: &["grok"],
        status: Some(&["models"]),
        first_line: true,
        env: &[("GROK_DISABLE_AUTOUPDATER", "1")],
        read: read_grok,
        login: "login",
        install: "curl -fsSL https://x.ai/cli/install.sh | bash",
        docs: "https://github.com/xai-org/grok-build",
    },
    Harness {
        id: "opencode",
        name: "OpenCode",
        programs: &["opencode"],
        status: Some(&["auth", "list", "--format", "json"]),
        first_line: false,
        env: &[],
        read: read_opencode,
        login: "auth login",
        install: "curl -fsSL https://opencode.ai/install | bash",
        docs: "https://opencode.ai/docs/",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub auth: Auth,
    pub account: Option<String>,
    /// Why the sign-in state is unknown.
    pub note: Option<String>,
    pub login: String,
    pub install: String,
    pub docs: String,
}

/// Where a CLI is, by its name.
pub type Find = dyn Fn(&str) -> Option<PathBuf> + Sync;

/// Where the checks find the CLIs and what they run them with: the login shell's PATH, which node-based CLIs need to
/// find `node`, and the desktop's timeout. Tests point it at a folder of fake CLIs.
pub struct Lookup {
    pub find: Box<Find>,
    pub path: String,
    pub timeout: Duration,
}

impl Lookup {
    /// Yours: the programs on the login shell's PATH, as the desktop finds them.
    pub fn system() -> Self {
        Lookup {
            find: Box::new(wings_plugins::exec::find_program),
            path: wings_plugins::exec::search_path(),
            timeout: TIMEOUT,
        }
    }
}

fn read_claude(code: Option<i32>, out: &str) -> SignIn {
    match code {
        Some(0) => (
            Auth::SignedIn,
            serde_json::from_str::<Value>(out)
                .ok()
                .and_then(|v| v["email"].as_str().map(String::from)),
        ),
        Some(1) => (Auth::SignedOut, None),
        _ => (Auth::Unknown, None),
    }
}

fn read_codex(code: Option<i32>, out: &str) -> SignIn {
    let line = |prefix: &str| {
        out.lines().find_map(|l| {
            l.trim()
                .strip_prefix(prefix)
                .map(|rest| rest.trim().to_string())
        })
    };
    match code {
        Some(0) => (
            Auth::SignedIn,
            line("Logged in using ")
                .or_else(|| line("API key configured").map(|_| "API key".into())),
        ),
        _ if out.contains("Not logged in") => (Auth::SignedOut, None),
        _ => (Auth::Unknown, None),
    }
}

fn read_grok(_: Option<i32>, out: &str) -> SignIn {
    let line = out.trim();
    if let Some(host) = line.strip_prefix("You are logged in with ") {
        (Auth::SignedIn, Some(host.trim_end_matches('.').to_string()))
    } else if line.starts_with("You are using XAI_API_KEY") {
        (Auth::SignedIn, Some("XAI_API_KEY".into()))
    } else if line.contains("authenticated via deployment key") {
        (Auth::SignedIn, Some("deployment key".into()))
    } else if line.contains("not authenticated") {
        (Auth::SignedOut, None)
    } else {
        (Auth::Unknown, None)
    }
}

/// Signed in when any provider has stored credentials. OpenCode keeps no account of its own, so the account is those
/// providers.
fn read_opencode(code: Option<i32>, out: &str) -> SignIn {
    let Some(Value::Array(providers)) = (code == Some(0))
        .then(|| serde_json::from_str(out).ok())
        .flatten()
    else {
        return (Auth::Unknown, None);
    };
    if providers.is_empty() {
        return (Auth::SignedOut, None);
    }
    let names: Vec<&str> = providers
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    (Auth::SignedIn, Some(names.join(", ")))
}

/// The version number in a `--version` line: `2.1.312 (Claude Code)` and `opencode v2.1.4` give `2.1.312` and `2.1.4`.
fn version(out: &str) -> Option<String> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let word = line
        .split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()) && w.contains('.'));
    Some(word.unwrap_or(line).to_string())
}

/// Runs a CLI with `lookup`'s PATH. Gives the exit code and stdout then stderr, or nothing when it didn't answer in
/// time. With `first_line`, stops at the first line of stdout and ends the program.
fn run(
    lookup: &Lookup,
    path: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    first_line: bool,
) -> Option<(Option<i32>, String)> {
    let mut child = Command::new(path)
        .args(args)
        .env("PATH", &lookup.path)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let (stdout, stderr) = (child.stdout.take()?, child.stderr.take()?);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let text = if first_line {
            BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
        } else {
            let mut text = String::new();
            let _ = BufReader::new(stdout).read_to_string(&mut text);
            text
        };
        let _ = tx.send(text);
    });
    let (etx, erx) = mpsc::channel();
    thread::spawn(move || {
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text);
        let _ = etx.send(text);
    });
    let start = Instant::now();
    let out = rx.recv_timeout(lookup.timeout).ok();
    if first_line || out.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        return out.map(|o| (None, o));
    }
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if start.elapsed() < lookup.timeout => {
                thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let err = erx
        .recv_timeout(Duration::from_millis(200))
        .unwrap_or_default();
    Some((code, format!("{}{err}", out.unwrap_or_default())))
}

fn check(lookup: &Lookup, h: &Harness) -> HarnessStatus {
    let found = h
        .programs
        .iter()
        .find_map(|p| (lookup.find)(p).map(|path| (*p, path)));
    let program = found.as_ref().map_or(h.programs[0], |(p, _)| *p);
    let mut status = HarnessStatus {
        id: h.id.into(),
        name: h.name.into(),
        installed: found.is_some(),
        version: None,
        auth: Auth::Unknown,
        account: None,
        note: None,
        login: format!("{program} {}", h.login),
        install: h.install.into(),
        docs: h.docs.into(),
    };
    let Some((_, path)) = found else {
        return status;
    };
    // Both at once: a CLI that takes a second to start would otherwise take two.
    thread::scope(|s| {
        let version = s.spawn(|| {
            run(lookup, &path, &["--version"], h.env, false).and_then(|(_, out)| version(&out))
        });
        match h.status {
            Some(args) => match run(lookup, &path, args, h.env, h.first_line) {
                Some((code, out)) => (status.auth, status.account) = (h.read)(code, &out),
                None => {
                    status.note = Some(format!(
                        "{program} didn't answer in {}s",
                        lookup.timeout.as_secs()
                    ))
                }
            },
            None => status.note = Some("Installed. Wings can't check its sign-in yet".into()),
        }
        status.version = version.join().ok().flatten();
    });
    if status.auth == Auth::Unknown && status.note.is_none() {
        status.note = Some("Installed. Its sign-in check gave an answer Wings doesn't know".into());
    }
    status
}

/// Every harness, checked side by side, in table order. Runs their CLIs, so it takes a few seconds: never on the
/// window's thread.
pub fn check_all(lookup: &Lookup) -> Vec<HarnessStatus> {
    thread::scope(|s| {
        HARNESSES
            .iter()
            .map(|h| s.spawn(move || check(lookup, h)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_the_exit_code_and_email() {
        let out = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"email\": \"me@example.com\"\n}";
        assert_eq!(
            read_claude(Some(0), out),
            (Auth::SignedIn, Some("me@example.com".into()))
        );
        assert_eq!(
            read_claude(Some(1), "{\"loggedIn\": false}"),
            (Auth::SignedOut, None)
        );
        assert_eq!(read_claude(None, ""), (Auth::Unknown, None));
    }

    #[test]
    fn codex_reads_how_you_signed_in() {
        assert_eq!(
            read_codex(Some(0), "Logged in using ChatGPT\n"),
            (Auth::SignedIn, Some("ChatGPT".into()))
        );
        assert_eq!(
            read_codex(Some(0), "API key configured - sk-…abcd\nAuthentication is checked when you send a request.\n"),
            (Auth::SignedIn, Some("API key".into()))
        );
        assert_eq!(
            read_codex(Some(1), "Not logged in\n"),
            (Auth::SignedOut, None)
        );
        assert_eq!(
            read_codex(Some(1), "Error checking login status: boom\n"),
            (Auth::Unknown, None)
        );
    }

    #[test]
    fn grok_reads_its_first_line() {
        assert_eq!(
            read_grok(None, "You are logged in with auth.x.ai."),
            (Auth::SignedIn, Some("auth.x.ai".into()))
        );
        assert_eq!(
            read_grok(None, "You are using XAI_API_KEY."),
            (Auth::SignedIn, Some("XAI_API_KEY".into()))
        );
        assert_eq!(
            read_grok(None, "You are not authenticated."),
            (Auth::SignedOut, None)
        );
        assert_eq!(read_grok(None, "Default model: x"), (Auth::Unknown, None));
    }

    #[test]
    fn opencode_is_signed_in_with_any_provider() {
        let out = r#"[{"id":"opencode-go","name":"OpenCode Go","connections":[]},{"id":"openai","name":"OpenAI","connections":[]}]"#;
        assert_eq!(
            read_opencode(Some(0), out),
            (Auth::SignedIn, Some("OpenCode Go, OpenAI".into()))
        );
        assert_eq!(read_opencode(Some(0), "[]"), (Auth::SignedOut, None));
        assert_eq!(read_opencode(Some(0), "not json"), (Auth::Unknown, None));
    }

    #[test]
    fn version_finds_the_number() {
        assert_eq!(
            version("2.1.312 (Claude Code)\n").as_deref(),
            Some("2.1.312")
        );
        assert_eq!(version("opencode v2.1.4\n").as_deref(), Some("2.1.4"));
        assert_eq!(version("codex-cli 0.48.0").as_deref(), Some("0.48.0"));
        assert_eq!(version("dev build").as_deref(), Some("dev build"));
        assert_eq!(version(""), None);
    }

    /// A folder of fake CLIs, and a lookup that finds programs only there, never yours.
    #[cfg(unix)]
    fn fakes(name: &str, scripts: &[(&str, &str)], timeout: Duration) -> (Lookup, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "wings-connect-harnesses-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (program, body) in scripts {
            let path = dir.join(program);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let only = dir.clone();
        let lookup = Lookup {
            find: Box::new(move |p| Some(only.join(p)).filter(|p| p.is_file())),
            path: format!("{}:/usr/bin:/bin", dir.display()),
            timeout,
        };
        (lookup, dir)
    }

    #[cfg(unix)]
    #[test]
    fn each_cli_is_found_versioned_and_asked_whether_you_are_signed_in() {
        let claude = r#"case "$1" in
--version) echo "2.1.312 (Claude Code)";;
auth) echo '{"loggedIn": true, "email": "me@example.com"}';;
esac"#;
        // An old Codex, signed out: its version is shown as it says it, and it exits 1.
        let codex = r#"case "$1" in
--version) echo "codex-cli 0.1.0";;
login) echo "Not logged in"; exit 1;;
esac"#;
        // `agent` is Cursor's newer name; it has no sign-in check.
        let agent = r#"[ "$1" = --version ] && echo "2025.10.1""#;
        // The PATH the checks give it is the lookup's.
        let opencode = r#"case "$1" in
--version) case ":$PATH:" in *:/usr/bin:*) echo "opencode v2.1.4";; *) echo "no PATH";; esac;;
auth) echo '[]';;
esac"#;
        let (lookup, dir) = fakes(
            "all",
            &[
                ("claude", claude),
                ("codex", codex),
                ("agent", agent),
                ("opencode", opencode),
            ],
            TIMEOUT,
        );
        let list = check_all(&lookup);
        let ids: Vec<_> = list.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "cursor", "grok", "opencode"]);
        let [claude, codex, cursor, grok, opencode] = list.try_into().unwrap();
        assert!(claude.installed);
        assert_eq!(claude.version.as_deref(), Some("2.1.312"));
        assert_eq!(
            (claude.auth, claude.account.as_deref()),
            (Auth::SignedIn, Some("me@example.com"))
        );
        assert_eq!(claude.login, "claude auth login");
        assert_eq!(
            (codex.version.as_deref(), codex.auth),
            (Some("0.1.0"), Auth::SignedOut)
        );
        assert_eq!(codex.note, None);
        assert!(cursor.installed);
        assert_eq!(
            cursor.login, "agent login",
            "the program found is the one to sign in with"
        );
        assert_eq!(cursor.auth, Auth::Unknown);
        assert_eq!(
            cursor.note.as_deref(),
            Some("Installed. Wings can't check its sign-in yet")
        );
        assert!(!grok.installed);
        assert_eq!(
            (grok.version, grok.auth, grok.note),
            (None, Auth::Unknown, None)
        );
        assert_eq!(
            grok.install,
            "curl -fsSL https://x.ai/cli/install.sh | bash"
        );
        assert_eq!(opencode.version.as_deref(), Some("2.1.4"));
        assert_eq!(opencode.auth, Auth::SignedOut);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_that_hangs_is_given_up_on_after_the_timeout() {
        // Answers its version, then never its sign-in; and Grok's first line ends it without waiting for the rest.
        let claude = r#"[ "$1" = --version ] && { echo "2.0.0"; exit 0; }
exec sleep 30"#;
        let grok = r#"[ "$1" = --version ] && { echo "grok 1.0.0"; exit 0; }
echo "You are logged in with auth.x.ai."
exec sleep 30"#;
        let (lookup, dir) = fakes(
            "hang",
            &[("claude", claude), ("grok", grok)],
            Duration::from_secs(1),
        );
        let started = Instant::now();
        let list = check_all(&lookup);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        let claude = &list[0];
        assert_eq!(claude.version.as_deref(), Some("2.0.0"));
        assert_eq!(claude.auth, Auth::Unknown);
        assert_eq!(claude.note.as_deref(), Some("claude didn't answer in 1s"));
        let grok = &list[3];
        assert_eq!(
            (grok.auth, grok.account.as_deref()),
            (Auth::SignedIn, Some("auth.x.ai"))
        );
        assert_eq!(grok.version.as_deref(), Some("1.0.0"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
