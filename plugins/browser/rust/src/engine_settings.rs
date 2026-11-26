//! The Engine section of the Browser's settings: WebKit, which is built in, or Chromium, which runs Chrome extensions
//! like Bitwarden and is downloaded once, on the person's click. Wings (`webEngine`) fetches, checks and unpacks it; the
//! page calls each step in turn so the row can say which one it is on. No percentage and no timer: the row's words change
//! three times and its spinner is the app's, so nothing redraws the page while it works.

use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::{
    host,
    task::spawn,
    ui::{Entry, Node, actions, badge, button, dialog, entry, error_state, icon, muted, rows_card, section, setting, spinner, text},
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    WebKit,
    Chromium,
}

/// What `webEngine` `status` says.
#[derive(Clone, Debug, PartialEq)]
struct Info {
    engine: Kind,
    installed: bool,
    download_bytes: u64,
    disk_bytes: u64,
    /// False on a Mac that can't run Chromium, with `why`.
    available: bool,
    why: Option<String>,
}

fn parse(answer: &Value) -> Info {
    Info {
        engine: if answer["engine"] == "chromium" { Kind::Chromium } else { Kind::WebKit },
        installed: answer["installed"].as_bool().unwrap_or_default(),
        download_bytes: answer["downloadBytes"].as_u64().unwrap_or_default(),
        disk_bytes: answer["diskBytes"].as_u64().unwrap_or_default(),
        available: answer["available"].as_bool().unwrap_or(true),
        why: answer["why"].as_str().map(str::to_owned),
    }
}

/// Where setting Chromium up is: each is a milestone the row says.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Download,
    Verify,
    Install,
}

#[derive(Default)]
struct State {
    info: Option<Result<Info, String>>,
    step: Option<Step>,
    /// Why the last setting up or switching failed: shown with Try again.
    failed: Option<String>,
    confirm_remove: bool,
    removing: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(change: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| change(&mut state.borrow_mut()))
}

/// Megabytes as the row says them: a million bytes, rounded.
fn mb(bytes: u64) -> u64 {
    (bytes + 500_000) / 1_000_000
}

pub fn load() {
    spawn(async {
        let info = host::web_engine_status().await.map(|answer| parse(&answer));
        with(|state| state.info = Some(info));
    });
}

/// Downloads, checks and installs Chromium, then switches to it. Runs in the click's turn: the host only starts a
/// download right after one.
fn get_chromium() {
    if with(|state| std::mem::replace(&mut state.step, Some(Step::Download)).is_some()) {
        return;
    }
    with(|state| state.failed = None);
    spawn(async {
        let result = async {
            host::web_engine_download().await?;
            with(|state| state.step = Some(Step::Verify));
            host::web_engine_verify().await?;
            with(|state| state.step = Some(Step::Install));
            host::web_engine_install().await?;
            host::web_engine_use("chromium").await
        }
        .await;
        with(|state| {
            state.step = None;
            state.failed = result.err();
        });
        load();
    });
}

/// Chooses the engine the Browser's tabs use: they load again in it.
fn use_engine(kind: Kind) {
    with(|state| state.failed = None);
    spawn(async move {
        let result = host::web_engine_use(if kind == Kind::Chromium { "chromium" } else { "webkit" }).await;
        with(|state| state.failed = result.err());
        load();
    });
}

fn remove() {
    with(|state| {
        state.confirm_remove = false;
        state.removing = true;
        state.failed = None;
    });
    spawn(async {
        let result = host::web_engine_remove().await;
        with(|state| {
            state.removing = false;
            state.failed = result.err();
        });
        load();
    });
}

/// A choice's mark: a ring, with a tick in it for the engine in use.
fn mark(chosen: bool) -> Node {
    const RING: &str = "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z";
    if chosen { icon(24, 2.0, &[RING, "m9 12 2 2 4-4"]).prop("tone", "info") } else { icon(24, 2.0, &[RING]) }
}

fn row(name: &str, detail: Node, chosen: bool, control: Node) -> Node {
    entry(Entry { lead: Some(mark(chosen)), title: text(name).prop("weight", "medium"), detail: Some(detail), trailing: Some(control) })
}

pub fn view() -> Vec<Node> {
    with(|state| {
        let info = match &state.info {
            None => return vec![section("Engine", None, [rows_card([entry(Entry { lead: Some(spinner()), title: muted("Loading"), detail: None, trailing: None })])])],
            Some(Err(error)) => return vec![error_state("Couldn't read the browser engine", error, Some(("Try again", Box::new(load))))],
            Some(Ok(info)) => info,
        };
        let small = |words: String| muted(words).prop("size", "small");
        let in_use = || badge("In use", None);
        let webkit = row(
            "WebKit",
            small("Built in.".into()),
            info.engine == Kind::WebKit,
            if info.engine == Kind::WebKit { in_use() } else { button("Use", "outline", false, || use_engine(Kind::WebKit)).size("sm") },
        );
        let features = "Runs Chrome extensions like Bitwarden.";
        let (detail, control) = match (state.step, info) {
            (Some(Step::Download), _) => (format!("Downloading {} MB", mb(info.download_bytes)), button("Download", "outline", true, || {}).size("sm")),
            (Some(Step::Verify), _) => ("Verifying".to_owned(), button("Download", "outline", true, || {}).size("sm")),
            (Some(Step::Install), _) => ("Installing".to_owned(), button("Download", "outline", true, || {}).size("sm")),
            (None, info) if !info.available => (info.why.clone().unwrap_or_else(|| "Chromium can't run on this Mac".into()), button("Download", "outline", false, || {}).size("sm").disabled(true)),
            (None, info) if info.engine == Kind::Chromium => (format!("{features} Takes {} MB on disk.", mb(info.disk_bytes)), in_use()),
            (None, info) if info.installed => (format!("{features} Takes {} MB on disk.", mb(info.disk_bytes)), button("Use", "outline", false, || use_engine(Kind::Chromium)).size("sm")),
            (None, info) => (
                format!("{features} Downloads {} MB once, {} MB on disk.", mb(info.download_bytes), mb(info.disk_bytes)),
                button("Download", "outline", false, get_chromium).size("sm"),
            ),
        };
        let chromium = row("Chromium", small(detail), info.engine == Kind::Chromium, control);
        let mut rows = vec![webkit, chromium];
        if info.installed {
            let remove = button("Remove", "outline", state.removing, || with(|state| state.confirm_remove = true)).size("sm");
            rows.push(setting("Remove Chromium", "Deletes it and what it kept for sites.", remove));
        }
        let mut out = vec![section("Engine", Some("Sign-ins and site data don't carry over between engines."), [rows_card(rows)])];
        if let Some(error) = &state.failed {
            let retry: Box<dyn Fn()> = if info.installed { Box::new(load) } else { Box::new(get_chromium) };
            out.push(error_state("Couldn't set up Chromium", error, Some(("Try again", retry))));
        }
        out
    })
}

/// Asks before Chromium goes, as a dialog over the page wherever the page is scrolled to.
pub fn remove_dialog() -> Option<Node> {
    if !with(|state| state.confirm_remove) {
        return None;
    }
    let cancel = || with(|state| state.confirm_remove = false);
    Some(
        dialog("Remove Chromium?", 420, cancel)
            .child(muted("Browser tabs switch back to WebKit, and your Chromium sign-ins and site data are deleted."))
            .child(actions(Some(button("Cancel", "ghost", false, cancel)), [button("Remove", "destructive", false, remove)])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell as Cell, rc::Rc};
    use wings_plugin::host::testing;

    fn status(engine: &str, installed: bool, available: bool) -> Value {
        json!({ "engine": engine, "installed": installed, "version": "156.0.31", "downloadBytes": 147_328_901u64, "diskBytes": 343_012_256u64,
            "available": available, "why": if available { Value::Null } else { json!("Chromium needs a Mac with Apple silicon") } })
    }

    fn drawn() -> String {
        wings_plugin::ui::export(view().into_iter().chain(remove_dialog()).collect::<Vec<_>>()).0.to_string()
    }

    fn shows(drawn: &str, words: &[&str]) {
        for word in words {
            assert!(drawn.contains(word), "{word} is not in {drawn}");
        }
    }

    /// Answers `webEngine` calls with `status` for the status, null for the rest, and the error for an action named in
    /// `fail`; keeps what was called, with the row's words at that moment.
    fn host_answers(status: Value, fail: Option<(&'static str, &'static str)>) -> Rc<Cell<Vec<(String, String)>>> {
        let calls = Rc::new(Cell::new(Vec::new()));
        let seen = calls.clone();
        testing::answer(move |method, params| {
            assert_eq!(method, "webEngine");
            let action = params["action"].as_str().unwrap_or_default().to_owned();
            let words = match with(|state| state.step) {
                Some(Step::Download) => "Downloading",
                Some(Step::Verify) => "Verifying",
                Some(Step::Install) => "Installing",
                None => "",
            };
            let label = if action == "use" { format!("use {}", params["engine"].as_str().unwrap_or_default()) } else { action.clone() };
            seen.borrow_mut().push((label, words.to_owned()));
            match fail {
                Some((failing, error)) if failing == action => Err(error.into()),
                _ if action == "status" => Ok(status.clone()),
                _ => Ok(Value::Null),
            }
        });
        calls
    }

    #[test]
    fn the_status_is_read_and_a_missing_field_is_not_a_reason_to_hide_the_section() {
        let info = parse(&status("chromium", true, true));
        assert_eq!((info.engine, info.installed, info.download_bytes, info.disk_bytes, info.available), (Kind::Chromium, true, 147_328_901, 343_012_256, true));
        assert_eq!(parse(&json!({})).engine, Kind::WebKit);
        let off = parse(&status("webkit", false, false));
        assert_eq!((off.available, off.why.as_deref()), (false, Some("Chromium needs a Mac with Apple silicon")));
    }

    #[test]
    fn with_chromium_not_installed_webkit_is_in_use_and_chromium_offers_a_download_with_the_real_sizes() {
        host_answers(status("webkit", false, true), None);
        load();
        testing::settle();
        let now = drawn();
        shows(&now, &["Engine", "Sign-ins and site data don't carry over between engines.", "WebKit", "Built in.", "In use", "Chromium", "Runs Chrome extensions like Bitwarden. Downloads 147 MB once, 343 MB on disk.", "Download"]);
        assert_eq!(now.matches("In use").count(), 1, "{now}");
        assert!(!now.contains("Remove Chromium"), "{now}");
    }

    #[test]
    fn setting_chromium_up_calls_each_step_in_turn_says_each_milestone_and_ends_on_chromium() {
        let calls = host_answers(status("webkit", false, true), None);
        load();
        testing::settle();
        get_chromium();
        // Before the first answer the row says the download.
        assert!(drawn().contains("Downloading 147 MB"), "{}", drawn());
        testing::settle();
        let sent = calls.borrow();
        let steps: Vec<(&str, &str)> = sent.iter().map(|(call, words)| (call.as_str(), words.as_str())).collect();
        assert_eq!(
            steps,
            [("status", ""), ("download", "Downloading"), ("verify", "Verifying"), ("install", "Installing"), ("use chromium", "Installing"), ("status", "")]
        );
        assert_eq!(with(|state| state.step), None);
    }

    #[test]
    fn a_step_that_fails_stops_there_and_shows_the_error_with_try_again() {
        let error = "The download didn't match what Wings expects, so it was deleted. Try again.";
        let calls = host_answers(status("webkit", false, true), Some(("verify", error)));
        load();
        testing::settle();
        get_chromium();
        testing::settle();
        let now = drawn();
        shows(&now, &["Couldn't set up Chromium", error, "Try again", "danger"]);
        let done: Vec<String> = calls.borrow().iter().map(|(call, _)| call.clone()).collect();
        assert!(!done.contains(&"install".to_owned()) && !done.contains(&"use chromium".to_owned()), "{done:?}");
        // Try again starts over from the download.
        host_answers(status("webkit", false, true), None);
        get_chromium();
        testing::settle();
        assert!(!drawn().contains("Couldn't set up Chromium"), "{}", drawn());
    }

    #[test]
    fn a_second_press_while_it_downloads_starts_nothing() {
        let calls = host_answers(status("webkit", false, true), None);
        get_chromium();
        get_chromium();
        testing::settle();
        assert_eq!(calls.borrow().iter().filter(|(call, _)| call == "download").count(), 1);
    }

    #[test]
    fn with_chromium_on_it_is_marked_and_there_is_a_remove_row_that_asks_first() {
        let calls = host_answers(status("chromium", true, true), None);
        load();
        testing::settle();
        let now = drawn();
        shows(&now, &["Remove Chromium", "Deletes it and what it kept for sites.", "\"Remove\"", "\"Use\""]);
        assert!(!now.contains("Remove Chromium?"), "{now}");
        // The WebKit row offers Use, and it asks for webkit.
        use_engine(Kind::WebKit);
        testing::settle();
        assert!(calls.borrow().iter().any(|(call, _)| call == "use webkit"));
        // Remove opens a dialog with the words, and only its Remove calls the host.
        with(|state| state.confirm_remove = true);
        let now = drawn();
        shows(&now, &["\"Dialog\"", "Remove Chromium?", "Browser tabs switch back to WebKit, and your Chromium sign-ins and site data are deleted.", "\"Cancel\"", "\"destructive\""]);
        assert!(!calls.borrow().iter().any(|(call, _)| call == "remove"));
        remove();
        testing::settle();
        assert!(calls.borrow().iter().any(|(call, _)| call == "remove"));
        assert!(!drawn().contains("Remove Chromium?"));
    }

    #[test]
    fn with_chromium_installed_but_webkit_chosen_chromium_offers_use_and_the_remove_row_stays() {
        host_answers(status("webkit", true, true), None);
        load();
        testing::settle();
        let now = drawn();
        shows(&now, &["Takes 343 MB on disk.", "\"Use\"", "Remove Chromium"]);
        assert!(!now.contains("Downloads 147 MB"), "{now}");
    }

    #[test]
    fn on_a_mac_that_cant_run_chromium_its_row_is_disabled_and_says_why() {
        host_answers(status("webkit", false, false), None);
        load();
        testing::settle();
        let now = drawn();
        shows(&now, &["Chromium needs a Mac with Apple silicon", "\"disabled\":true"]);
        assert!(!now.contains("Downloads 147 MB"), "{now}");
    }

    #[test]
    fn a_status_that_cant_be_read_says_so_with_try_again() {
        testing::answer(|_, _| Err("nope".into()));
        load();
        testing::settle();
        shows(&drawn(), &["Couldn't read the browser engine", "nope", "Try again"]);
    }
}
