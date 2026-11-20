//! Which Claude Code the chats run, whether a newer one is out, and the button that runs `claude upgrade`. Shown in the
//! Claude chat page in Wings Settings (`settings_page.rs`).
//!
//! The installed version is what `claude --version` prints. The newest is what Claude Code itself asks for on a native
//! install: a plain-text version at `https://downloads.claude.ai/claude-code-releases/latest`.

use std::cell::RefCell;
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, badge, button, notice, row, spinner, stack, text},
};

/// Where the newest version is, as plain text, and the pages it may be read from (`permissions.fetch`).
const LATEST_URL: &str = "https://downloads.claude.ai/claude-code-releases/latest";

/// How long a read of the newest version is kept while the page stays open.
const KEEP_MS: f64 = 3_600_000.0;

/// `claude upgrade` downloads a build: five minutes is the most a call may wait.
const UPGRADE_MS: u32 = 300_000;

/// The longest error text shown, so a stack of output doesn't fill the page.
const ERROR_CHARS: usize = 300;

#[derive(Clone, Debug, Default, PartialEq)]
struct State {
    /// What `claude --version` printed, the version only.
    current: Option<String>,
    /// The newest version, once read.
    latest: Option<String>,
    /// When `latest` was read, in the clock's milliseconds.
    read_at: Option<f64>,
    checking: bool,
    upgrading: bool,
    /// How the last click on the button ended: the line `claude upgrade` ended with, or why it failed.
    outcome: Option<Result<String, String>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// How the installed version stands against the newest.
#[derive(Debug, PartialEq)]
enum Standing {
    UpToDate,
    Available(String),
    Unknown,
}

/// The version in `claude --version`'s line, like `2.1.329 (Claude Code)`, when it starts with one.
fn parse_version(line: &str) -> Option<String> {
    let version = line.split_whitespace().next()?;
    numbers(version).map(|_| version.to_owned())
}

/// A version's numbers, `None` when it isn't dotted digits alone.
fn numbers(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<u64>>>().filter(|parts| parts.len() >= 2)
}

fn standing(current: Option<&str>, latest: Option<&str>) -> Standing {
    let (Some(current), Some(latest)) = (current, latest) else {
        return Standing::Unknown;
    };
    match (numbers(current), numbers(latest)) {
        (Some(have), Some(newest)) if newest > have => Standing::Available(latest.to_owned()),
        (Some(_), Some(_)) => Standing::UpToDate,
        _ => Standing::Unknown,
    }
}

fn printed(out: Result<serde_json::Value, String>) -> Result<String, String> {
    let out = out?;
    let stdout = out["stdout"].as_str().unwrap_or_default().trim();
    if out["code"] == 0 {
        return Ok(stdout.to_owned());
    }
    let stderr = out["stderr"].as_str().unwrap_or_default().trim();
    let said = if stderr.is_empty() { stdout } else { stderr };
    Err(said.chars().take(ERROR_CHARS).collect::<String>())
}

/// The line `claude upgrade` ended with: what it did, or that nothing was newer.
fn last_line(output: &str) -> String {
    output.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or("Done").to_owned()
}

fn with<T>(change: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| change(&mut state.borrow_mut()))
}

/// Reads the installed version and the newest, unless the newest was read within the hour. Runs when the page opens.
pub fn check() {
    let now = host::now();
    let go = with(|state| {
        let fresh = state.read_at.is_some_and(|at| now - at < KEEP_MS);
        if state.checking || state.upgrading || fresh && state.current.is_some() {
            return false;
        }
        state.checking = true;
        true
    });
    if !go {
        return;
    }
    spawn(async move {
        let current = printed(host::exec("claude", &["--version"]).await).ok().and_then(|line| parse_version(&line));
        let latest = match host::fetch(LATEST_URL, "GET", None, None).await {
            Ok(answer) if answer.status == 200 => numbers(answer.body.trim()).map(|_| answer.body.trim().to_owned()),
            _ => None,
        };
        with(|state| {
            state.checking = false;
            state.current = current;
            state.read_at = latest.is_some().then_some(now);
            state.latest = latest;
        });
    });
}

/// Runs `claude upgrade` from a click on its button, then reads the version again.
fn upgrade() {
    if !with(|state| {
        if state.upgrading {
            return false;
        }
        state.upgrading = true;
        state.outcome = None;
        true
    }) {
        return;
    }
    spawn(async move {
        let done = printed(host::exec_timeout("claude", &["upgrade"], UPGRADE_MS).await);
        let current = printed(host::exec("claude", &["--version"]).await).ok().and_then(|line| parse_version(&line));
        with(|state| {
            state.upgrading = false;
            if current.is_some() {
                state.current = current;
            }
            state.outcome = Some(done.map(|output| last_line(&output)));
        });
    });
}

/// The line and badge for where the installed version stands.
fn headline(state: &State) -> Node {
    let name = match &state.current {
        Some(version) => format!("Claude Code {version}"),
        None if state.checking => "Claude Code".to_owned(),
        None => "Claude Code is not installed, or its version could not be read".to_owned(),
    };
    let mut line = row().prop("gap", "s").prop("wrap", true).child(text(name).prop("weight", "medium"));
    if state.checking && state.current.is_none() {
        return line.child(spinner());
    }
    line = match standing(state.current.as_deref(), state.latest.as_deref()) {
        Standing::UpToDate => line.child(badge("Up to date", Some("success"))),
        Standing::Available(version) => line.child(badge(format!("{version} available"), Some("warn"))),
        Standing::Unknown if state.current.is_some() => {
            line.child(badge("Newest version unknown", None).prop("title", "The newest version could not be read, so this can't say whether it is up to date"))
        }
        Standing::Unknown => line,
    };
    line
}

/// What the button changes: a stack of the version line, the button and how its last run ended.
pub fn section() -> Node {
    let state = with(|state| state.clone());
    let available = matches!(standing(state.current.as_deref(), state.latest.as_deref()), Standing::Available(_));
    let variant = if available { "default" } else { "outline" };
    let update = button(if state.upgrading { "Updating Claude Code" } else { "Update Claude Code" }, variant, state.upgrading, upgrade)
        .prop("title", "Runs `claude upgrade`")
        .key("update-claude-code");
    let mut section = stack().prop("gap", "s").child(headline(&state)).child(row().child(update));
    section = match &state.outcome {
        Some(Ok(line)) => section.child(notice("success", format!("{line}. Chats that are open keep running the version they started with until they restart."))),
        Some(Err(why)) => section.child(notice("danger", format!("Claude Code could not update: {why}"))),
        None => section,
    };
    section
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_version_is_the_first_word_of_what_claude_prints() {
        assert_eq!(parse_version("2.1.329 (Claude Code)\n").as_deref(), Some("2.1.329"));
        assert_eq!(parse_version("2.1.329").as_deref(), Some("2.1.329"));
        assert_eq!(parse_version("claude: command not found"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn versions_compare_by_their_numbers_not_their_text() {
        assert_eq!(standing(Some("2.1.329"), Some("2.1.331")), Standing::Available("2.1.331".into()));
        assert_eq!(standing(Some("2.1.99"), Some("2.1.100")), Standing::Available("2.1.100".into()));
        assert_eq!(standing(Some("2.1.329"), Some("2.1.329")), Standing::UpToDate);
        // Ahead of the channel it was read from, as a build from another channel can be.
        assert_eq!(standing(Some("2.1.329"), Some("2.1.311")), Standing::UpToDate);
        assert_eq!(standing(None, Some("2.1.329")), Standing::Unknown);
        assert_eq!(standing(Some("2.1.329"), Some("<html>")), Standing::Unknown);
    }

    #[test]
    fn a_failed_command_gives_its_error_and_a_good_one_its_last_line() {
        assert_eq!(printed(Ok(json!({ "code": 0, "stdout": "a\nb\n", "stderr": "" }))), Ok("a\nb".into()));
        assert_eq!(printed(Ok(json!({ "code": 1, "stdout": "", "stderr": "EACCES: permission denied\n" }))), Err("EACCES: permission denied".into()));
        assert_eq!(printed(Err("claude is not installed or not on PATH".into())), Err("claude is not installed or not on PATH".into()));
        let long = "x".repeat(ERROR_CHARS + 50);
        assert_eq!(printed(Ok(json!({ "code": 1, "stdout": "", "stderr": long }))).unwrap_err().len(), ERROR_CHARS);
        assert_eq!(last_line("Checking for updates\nSuccessfully updated to 2.1.331\n\n"), "Successfully updated to 2.1.331");
        assert_eq!(last_line(""), "Done");
    }

    fn page(state: State) -> String {
        STATE.with(|kept| *kept.borrow_mut() = state);
        wings_plugin::ui::tree_json(section()).to_string()
    }

    #[test]
    fn the_button_is_primary_only_when_a_newer_version_is_out_and_busy_while_it_runs() {
        let known = State { current: Some("2.1.329".into()), latest: Some("2.1.329".into()), ..State::default() };
        let page_ok = page(known.clone());
        assert!(page_ok.contains("Claude Code 2.1.329") && page_ok.contains("Up to date") && page_ok.contains("\"variant\":\"outline\""));
        let newer = page(State { latest: Some("2.1.331".into()), ..known.clone() });
        assert!(newer.contains("2.1.331 available") && newer.contains("\"variant\":\"default\""));
        let busy = page(State { upgrading: true, ..known.clone() });
        assert!(busy.contains("Updating Claude Code") && busy.contains("\"disabled\":true"));
        let unread = page(State { latest: None, ..known });
        assert!(unread.contains("Newest version unknown"));
    }

    #[test]
    fn the_result_line_says_what_happened_and_that_open_chats_keep_the_old_version() {
        let known = State { current: Some("2.1.331".into()), latest: Some("2.1.331".into()), ..State::default() };
        let done = page(State { outcome: Some(Ok("Successfully updated to 2.1.331".into())), ..known.clone() });
        assert!(done.contains("Successfully updated to 2.1.331") && done.contains("until they restart"));
        let failed = page(State { outcome: Some(Err("EACCES".into())), ..known });
        assert!(failed.contains("Claude Code could not update: EACCES") && failed.contains("danger"));
    }

    #[test]
    fn a_click_runs_claude_upgrade_with_no_flags_and_then_reads_the_version_again() {
        let calls = std::rc::Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let seen = calls.clone();
        host::testing::answer(move |method, params| {
            assert_eq!(method, "exec");
            let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|arg| arg.as_str().unwrap().to_owned()).collect();
            seen.borrow_mut().push(std::iter::once(params["program"].as_str().unwrap().to_owned()).chain(args.clone()).collect());
            Ok(match args[0].as_str() {
                "upgrade" => json!({ "code": 0, "stdout": "Successfully updated to 2.1.331\n", "stderr": "" }),
                _ => json!({ "code": 0, "stdout": "2.1.331 (Claude Code)\n", "stderr": "" }),
            })
        });
        STATE.with(|kept| *kept.borrow_mut() = State { current: Some("2.1.329".into()), latest: Some("2.1.331".into()), ..State::default() });
        upgrade();
        wings_plugin::task::block_on(async {});
        let state = with(|state| state.clone());
        assert_eq!(*calls.borrow(), [vec!["claude", "upgrade"], vec!["claude", "--version"]]);
        assert_eq!(state.current.as_deref(), Some("2.1.331"));
        assert_eq!(state.outcome, Some(Ok("Successfully updated to 2.1.331".into())));
        assert!(!state.upgrading);
    }
}
