//! Running az, gh or glab from a plugin, as `wings-plugin-sdk/cli` does: tells "not installed", "signed out" and a plain
//! failure apart, and keeps what a pane badge needs to offer Install or Sign in. The plugin still lists each command in
//! `permissions.exec`.

use crate::{host, task::join_all};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin, rc::Rc};

/// How `wings.exec` rejects for a program that isn't installed.
const NOT_INSTALLED: &str = "is not installed or not on PATH";
/// The longest `wings.exec` allows, for a sign-in that waits on a browser.
const SIGN_IN_TIMEOUT_MS: u32 = 300_000;

/// The last line of `text`, which is the error az, gh and glab print after their warnings and context.
pub fn last_line(text: &str) -> &str {
    text.trim().lines().next_back().unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Missing,
    SignedOut,
    Failed,
}

/// What `run` and `json` fail with. `kind` says why, so a plugin doesn't match on text. `message` is all of stderr,
/// trimmed (`last_line` takes its last line, for a badge).
#[derive(Clone, Debug)]
pub struct CliError {
    pub kind: Kind,
    pub message: String,
}

impl From<CliError> for String {
    fn from(error: CliError) -> Self {
        error.message
    }
}

#[derive(Clone, PartialEq)]
struct Problem {
    kind: Kind,
    message: String,
}

/// What a badge says when the CLI can't be used.
pub struct ProblemText<'a> {
    pub title: &'a str,
    pub signed_out: &'a str,
    pub missing: &'a str,
}

type Retry = Rc<dyn Fn() -> Pin<Box<dyn Future<Output = ()>>>>;

struct State {
    program: &'static str,
    name: &'static str,
    signed_out: fn(&str) -> bool,
    sign_in: &'static [&'static str],
    /// key → what went wrong with it last.
    problems: RefCell<HashMap<String, Problem>>,
    /// `Some(key)` while a sign-in runs, `Some(None)` when it was started for no key in particular.
    signing: RefCell<Option<Option<String>>>,
    on_change: RefCell<Vec<Rc<dyn Fn()>>>,
    on_retry: RefCell<Vec<Retry>>,
}

/// A handle on one CLI. `run` and `json` call it. Unless `quiet`, a call records `Missing` and `SignedOut` under its
/// `key` (the Azure organization, or a provider and host), and a call that works clears it, so `problem_badge` says what
/// a pane should offer. A background read that must not turn badges into sign-in prompts is `quiet`. `signed_out` says
/// from stderr whether a failure is a signed-out one, and `sign_in` is the arguments of the program's login.
#[derive(Clone)]
pub struct Cli(Rc<State>);

impl Cli {
    pub fn new(program: &'static str, name: &'static str, signed_out: fn(&str) -> bool, sign_in: &'static [&'static str]) -> Self {
        Self(Rc::new(State {
            program,
            name,
            signed_out,
            sign_in,
            problems: RefCell::new(HashMap::new()),
            signing: RefCell::new(None),
            on_change: RefCell::new(Vec::new()),
            on_retry: RefCell::new(Vec::new()),
        }))
    }

    fn changed(&self) {
        for listener in self.0.on_change.borrow().clone() {
            listener();
        }
    }

    fn record(&self, key: &str, problem: Problem) {
        let known = self.0.problems.borrow_mut().insert(key.to_owned(), problem.clone());
        if known.as_ref() != Some(&problem) {
            self.changed();
        }
    }

    fn forget(&self, key: &str) {
        if self.0.problems.borrow_mut().remove(key).is_some() {
            self.changed();
        }
    }

    /// Runs the program and gives its stdout, or the `CliError` of its failure.
    pub async fn run(&self, args: &[String], key: &str, quiet: bool) -> Result<String, CliError> {
        let program = self.0.program;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = match host::exec(program, &args).await {
            Ok(out) => out,
            Err(message) => {
                let missing = message.contains(NOT_INSTALLED);
                if missing && !quiet {
                    self.record(key, Problem { kind: Kind::Missing, message: message.clone() });
                }
                return Err(CliError { kind: if missing { Kind::Missing } else { Kind::Failed }, message });
            }
        };
        if out["code"] == 0 {
            if !quiet {
                self.forget(key);
            }
            return Ok(out["stdout"].as_str().unwrap_or_default().to_owned());
        }
        let stderr = out["stderr"].as_str().unwrap_or_default();
        let text = stderr.trim();
        let signed_out = (self.0.signed_out)(stderr);
        if signed_out && !quiet {
            let message = last_line(text);
            self.record(key, Problem { kind: Kind::SignedOut, message: if message.is_empty() { format!("{program} failed") } else { message.to_owned() } });
        }
        Err(CliError { kind: if signed_out { Kind::SignedOut } else { Kind::Failed }, message: if text.is_empty() { format!("{program} failed") } else { text.to_owned() } })
    }

    /// `run`, parsed as JSON. A program that printed nothing, like `az boards query` for no rows, gives `empty`; without
    /// one that is a failure, so a result is never nothing.
    pub async fn json(&self, args: &[String], key: &str, quiet: bool, empty: Option<Value>) -> Result<Value, CliError> {
        let text = self.run(args, key, quiet).await?;
        if text.trim().is_empty() {
            return empty.ok_or_else(|| CliError { kind: Kind::Failed, message: format!("{} printed nothing", self.0.program) });
        }
        serde_json::from_str(&text).map_err(|error| CliError { kind: Kind::Failed, message: error.to_string() })
    }

    /// The badge a pane shows while `key` has a problem, or none when it has none.
    pub fn problem_badge(&self, key: &str, text: &ProblemText) -> Option<Value> {
        let problem = self.0.problems.borrow().get(key)?.clone();
        let retry = json!({ "id": "retry", "label": "Try again" });
        if problem.kind == Kind::Missing {
            return Some(json!({
                "tone": "warning", "title": text.title, "label": format!("Install {}", self.0.name), "subtitle": text.missing,
                "actions": [{ "id": "install", "label": "Install", "primary": true }, retry],
            }));
        }
        let busy = self.0.signing.borrow().as_ref().is_some_and(|signing| signing.as_deref().is_none_or(|signing| signing == key));
        Some(json!({
            "tone": "warning", "title": text.title,
            "label": if busy { "Signing in…".to_owned() } else { format!("Sign in to {}", text.title) },
            "loading": busy,
            "subtitle": if busy { "Finish signing in in your browser." } else { text.signed_out },
            "actions": [{ "id": "signin", "label": "Sign in", "primary": true }, retry],
        }))
    }

    /// Runs the program's login, for up to 5 minutes, then `retry`. One runs at a time: a call while another runs does
    /// nothing. A badge for `key` shows "Signing in…" meanwhile, and for any key when there's none.
    pub async fn sign_in(&self, key: Option<&str>) -> Result<(), String> {
        if self.0.signing.borrow().is_some() {
            return Ok(());
        }
        *self.0.signing.borrow_mut() = Some(key.map(str::to_owned));
        self.changed();
        let out = host::exec_timeout(self.0.program, self.0.sign_in, SIGN_IN_TIMEOUT_MS).await;
        let failed = match out {
            Ok(out) if out["code"] == 0 => None,
            Ok(out) => Some(Some(last_line(out["stderr"].as_str().unwrap_or_default()).to_owned()).filter(|line| !line.is_empty()).unwrap_or_else(|| "Sign-in failed".to_owned())),
            Err(error) => Some(error),
        };
        *self.0.signing.borrow_mut() = None;
        if let Some(error) = failed {
            self.changed();
            return Err(error);
        }
        self.retry().await;
        Ok(())
    }

    /// Asks Wings to install the program.
    pub async fn install(&self) -> Result<(), String> {
        host::call("installTool", json!({ "program": self.0.program })).await.map(drop)
    }

    /// Forgets every problem and has what failed for lack of the CLI or a sign-in read again, by the `on_retry` listeners.
    pub async fn retry(&self) {
        self.0.problems.borrow_mut().clear();
        let retries: Vec<_> = self.0.on_retry.borrow().iter().map(|retry| retry()).collect();
        join_all(retries).await;
        self.changed();
    }

    pub fn on_retry<Fut: Future<Output = ()> + 'static>(&self, listener: impl Fn() -> Fut + 'static) {
        self.0.on_retry.borrow_mut().push(Rc::new(move || Box::pin(listener())));
    }

    pub fn on_change(&self, listener: impl Fn() + 'static) {
        self.0.on_change.borrow_mut().push(Rc::new(listener));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::block_on;

    fn az() -> Cli {
        Cli::new("az", "Azure CLI", |stderr| stderr.to_lowercase().contains("login"), &["login"])
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_sign_in_problem_is_recorded_for_the_key_cleared_by_a_working_call_and_never_by_a_quiet_one() {
        let cli = az();
        host::testing::answer(|_, _| Ok(json!({ "code": 1, "stdout": "", "stderr": "WARNING: x\nERROR: Please run 'az login' to setup account.\n" })));
        let error = block_on({ let cli = cli.clone(); async move { cli.run(&args(&["boards"]), "org", false).await } }).unwrap_err();
        assert_eq!(error.kind, Kind::SignedOut);
        assert!(error.message.starts_with("WARNING: x"));
        let text = ProblemText { title: "Azure DevOps", signed_out: "Sign in.", missing: "Needs az." };
        assert_eq!(cli.problem_badge("org", &text).unwrap()["label"], "Sign in to Azure DevOps");
        assert!(cli.problem_badge("other", &text).is_none());
        host::testing::answer(|_, _| Ok(json!({ "code": 0, "stdout": "{}", "stderr": "" })));
        block_on({ let cli = cli.clone(); async move { cli.run(&args(&["boards"]), "org", true).await } }).unwrap();
        assert!(cli.problem_badge("org", &text).is_some());
        block_on({ let cli = cli.clone(); async move { cli.run(&args(&["boards"]), "org", false).await } }).unwrap();
        assert!(cli.problem_badge("org", &text).is_none());
    }

    #[test]
    fn a_program_that_isnt_installed_offers_install_and_empty_output_is_the_calls_empty() {
        let cli = az();
        host::testing::answer(|_, _| Err("az is not installed or not on PATH".into()));
        let error = block_on({ let cli = cli.clone(); async move { cli.run(&args(&["x"]), "org", false).await } }).unwrap_err();
        assert_eq!(error.kind, Kind::Missing);
        let text = ProblemText { title: "Azure DevOps", signed_out: "", missing: "Needs az." };
        let badge = cli.problem_badge("org", &text).unwrap();
        assert_eq!((badge["label"].as_str(), badge["actions"][0]["id"].as_str()), (Some("Install Azure CLI"), Some("install")));
        host::testing::answer(|_, _| Ok(json!({ "code": 0, "stdout": "", "stderr": "" })));
        let empty = block_on({ let cli = cli.clone(); async move { cli.json(&args(&["q"]), "org", true, Some(json!([]))).await } });
        assert_eq!(empty.unwrap(), json!([]));
        assert_eq!(block_on(async move { cli.json(&args(&["q"]), "org", true, None).await }).unwrap_err().message, "az printed nothing");
    }

    #[test]
    fn a_failed_sign_in_puts_the_button_back_and_a_good_one_reads_what_failed_again() {
        let cli = az();
        let retried = Rc::new(RefCell::new(0));
        let counted = retried.clone();
        cli.on_retry(move || { *counted.borrow_mut() += 1; async {} });
        host::testing::answer(|_, params| {
            if params["args"][0] == "login" { Ok(json!({ "code": 1, "stdout": "", "stderr": "ERROR: Login cancelled\n" })) } else { Ok(json!({ "code": 1, "stdout": "", "stderr": "run az login" })) }
        });
        let text = ProblemText { title: "Azure DevOps", signed_out: "", missing: "" };
        block_on({ let cli = cli.clone(); async move { cli.run(&args(&["x"]), "org", false).await } }).unwrap_err();
        assert_eq!(block_on({ let cli = cli.clone(); async move { cli.sign_in(None).await } }), Err("ERROR: Login cancelled".into()));
        assert_eq!(cli.problem_badge("org", &text).unwrap()["loading"], false);
        host::testing::answer(|_, _| Ok(json!({ "code": 0, "stdout": "", "stderr": "" })));
        block_on({ let cli = cli.clone(); async move { cli.sign_in(None).await } }).unwrap();
        assert_eq!(*retried.borrow(), 1);
        assert!(cli.problem_badge("org", &text).is_none());
    }
}
