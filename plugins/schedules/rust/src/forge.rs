//! Running `az`, `gh` and `glab` for what is assigned to you, and signing in to them. Each tells a program that isn't
//! installed and one that is signed out from a plain failure, which is what the page offers Install or Sign in for.

use serde_json::Value;
use std::cell::Cell;
use wings_plugin::{
    cli::{CliError, Kind, last_line},
    host,
};

/// How `wings.exec` rejects for a program that isn't installed.
const NOT_INSTALLED: &str = "is not installed or not on PATH";
/// The longest `wings.exec` allows, for a sign-in that waits on a browser.
const SIGN_IN_TIMEOUT_MS: u32 = 300_000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Program {
    Az,
    Gh,
    Glab,
}

thread_local! {
    static SIGNING: Cell<bool> = const { Cell::new(false) };
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `number` stands alone in `text`, like `\b401\b`.
fn has_number(text: &str, number: &str) -> bool {
    text.match_indices(number).any(|(at, _)| !text[..at].chars().next_back().is_some_and(word) && !text[at + number.len()..].chars().next().is_some_and(word))
}

/// What the key of a call is the host of: the part after the forge's name.
pub fn host_of(key: Option<&str>) -> &str {
    let key = key.unwrap_or_default();
    key.split_once(':').map_or(key, |(_, host)| host)
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Program::Az => "az",
            Program::Gh => "gh",
            Program::Glab => "glab",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        [Program::Az, Program::Gh, Program::Glab].into_iter().find(|program| program.name() == name)
    }

    /// Whether a failure's stderr says the program is signed out.
    async fn signed_out(self, stderr: &str, key: Option<&str>) -> bool {
        match self {
            Program::Az => stderr.to_lowercase().contains("login"),
            // No token prints `gh auth login` or GH_TOKEN; an expired one gh can renew prints "Try authenticating with: gh auth refresh".
            Program::Gh => {
                stderr.contains("gh auth login")
                    || stderr.contains("GH_TOKEN")
                    || stderr.match_indices("Try authenticating with:").any(|(at, found)| {
                        let rest = &stderr[at + found.len()..];
                        rest.starts_with(char::is_whitespace) && rest.trim_start().strip_prefix("gh auth refresh").is_some_and(|after| !after.starts_with(word))
                    })
            }
            // A private project looks missing (404) to a signed-out user, so ask glab whether it is signed in.
            Program::Glab => {
                has_number(stderr, "401")
                    || (has_number(stderr, "404") && host::exec("glab", &["auth", "status", "--hostname", host_of(key)]).await.is_ok_and(|out| out["code"] != 0))
            }
        }
    }

    fn sign_in_args(self, key: Option<&str>) -> Vec<String> {
        let words: Vec<&str> = match self {
            Program::Az => vec!["login", "--allow-no-subscriptions", "--output", "none"],
            Program::Gh => vec!["auth", "login", "--web", "--clipboard", "--hostname", host_of(key)],
            Program::Glab => vec!["auth", "login", "--web", "--hostname", host_of(key)],
        };
        words.into_iter().map(str::to_owned).collect()
    }

    /// The stdout of the program run in `cwd` as JSON; nothing printed is an empty list, like `az boards query` for no rows.
    pub async fn json(self, args: &[String], key: Option<&str>, cwd: &str) -> Result<Value, CliError> {
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = match host::exec_in(cwd, self.name(), &words).await {
            Ok(out) => out,
            Err(message) => {
                let kind = if message.contains(NOT_INSTALLED) { Kind::Missing } else { Kind::Failed };
                return Err(CliError { kind, message });
            }
        };
        let stdout = out["stdout"].as_str().unwrap_or_default();
        if out["code"] == 0 {
            if stdout.trim().is_empty() {
                return Ok(Value::Array(Vec::new()));
            }
            return serde_json::from_str(stdout).map_err(|error| CliError { kind: Kind::Failed, message: error.to_string() });
        }
        let stderr = out["stderr"].as_str().unwrap_or_default();
        let signed_out = self.signed_out(stderr, key).await;
        let text = stderr.trim();
        Err(CliError { kind: if signed_out { Kind::SignedOut } else { Kind::Failed }, message: if text.is_empty() { format!("{} failed", self.name()) } else { text.to_owned() } })
    }

    /// Runs the program's login for up to 5 minutes, and gives `on_line` each line it prints. One runs at a time: a call
    /// while another runs does nothing.
    pub async fn sign_in(self, key: Option<&str>, on_line: impl Fn(&str) + 'static) -> Result<(), String> {
        if SIGNING.with(|signing| signing.replace(true)) {
            return Ok(());
        }
        let args = self.sign_in_args(key);
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = host::exec_lines(self.name(), &words, SIGN_IN_TIMEOUT_MS, on_line).await;
        SIGNING.with(|signing| signing.set(false));
        let out = out?;
        if out["code"] != 0 {
            let line = last_line(out["stderr"].as_str().unwrap_or_default());
            return Err(if line.is_empty() { "Sign-in failed".into() } else { line.to_owned() });
        }
        Ok(())
    }

    /// Asks Wings to install the program.
    pub async fn install(self) -> Result<(), String> {
        host::call("installTool", serde_json::json!({ "program": self.name() })).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::task::block_on;

    fn fails(program: Program, stderr: &'static str) -> CliError {
        host::testing::answer(move |_, _| Ok(json!({ "code": 1, "stdout": "", "stderr": stderr })));
        block_on(async move { program.json(&[], Some("github:github.com"), "/work").await }).unwrap_err()
    }

    #[test]
    fn each_cli_tells_a_signed_out_failure_from_a_plain_one() {
        assert_eq!(fails(Program::Az, "ERROR: Please run 'az login' to setup account.").kind, Kind::SignedOut);
        assert_eq!(fails(Program::Gh, "To get started with GitHub CLI, please run:  gh auth login").kind, Kind::SignedOut);
        assert_eq!(fails(Program::Gh, "Try authenticating with:  gh auth refresh -s project").kind, Kind::SignedOut);
        assert_eq!(fails(Program::Gh, "Try authenticating with:  gh auth refreshing").kind, Kind::Failed);
        assert_eq!(fails(Program::Gh, "HTTP 500").kind, Kind::Failed);
        assert_eq!(fails(Program::Glab, "GET /issues: 401 Unauthorized").kind, Kind::SignedOut);
        assert_eq!(fails(Program::Glab, "GET /issues: 4010").kind, Kind::Failed);
    }

    #[test]
    fn a_404_is_signed_out_only_when_glab_says_it_is_not_signed_in() {
        host::testing::answer(|_, _| Ok(json!({ "code": 1, "stdout": "", "stderr": "404 Not Found" })));
        assert_eq!(block_on(async { Program::Glab.json(&[], Some("gitlab:gitlab.com"), "/work").await }).unwrap_err().kind, Kind::SignedOut);
        host::testing::answer(|_, params| Ok(json!({ "code": if params["args"][0] == "auth" { 0 } else { 1 }, "stdout": "", "stderr": "404 Not Found" })));
        assert_eq!(block_on(async { Program::Glab.json(&[], Some("gitlab:gitlab.com"), "/work").await }).unwrap_err().kind, Kind::Failed);
    }

    #[test]
    fn a_program_that_isnt_installed_is_missing_and_no_output_is_an_empty_list() {
        host::testing::answer(|_, _| Err("gh is not installed or not on PATH".into()));
        assert_eq!(block_on(async { Program::Gh.json(&[], None, "/work").await }).unwrap_err().kind, Kind::Missing);
        host::testing::answer(|_, _| Ok(json!({ "code": 0, "stdout": "\n", "stderr": "" })));
        assert_eq!(block_on(async { Program::Az.json(&[], None, "/work").await }).unwrap(), json!([]));
        assert_eq!(host_of(Some("github:github.com")), "github.com");
        assert_eq!(Program::Gh.sign_in_args(Some("github:ghe.example"))[5], "ghe.example");
    }
}
