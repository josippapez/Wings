//! The PR tracker's page in Wings Settings: what the sidebar lists, what you're notified about and how often to check,
//! and the tokens for the hosts that have no CLI to sign in with. A token goes to the system keychain and can be sent by
//! `fetch` but never read back, so this page only shows whether one is saved.

use crate::{
    ipc, prefs,
    watch::{NOTIFY, SIDEBAR, WATCH_MINUTES},
};
use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, button, checkbox, confirm, heading, muted, row, select, stack, text, text_input},
};

struct Host {
    secret: &'static str,
    name: &'static str,
    help: &'static str,
}

const HOSTS: [Host; 2] = [
    Host {
        secret: "bitbucket",
        name: "Bitbucket Cloud",
        help: "In your Atlassian account, open Security, Create and manage API tokens, Create API token with scopes, and pick Bitbucket. Give it the read:pullrequest:bitbucket and read:user:bitbucket scopes.",
    },
    Host { secret: "codeberg", name: "Codeberg", help: "Create an access token in your Codeberg account settings with the read:repository, read:issue and read:user scopes." },
];

#[derive(Default, Clone)]
struct Token {
    /// Whether a token is saved, once the keychain has said.
    saved: Option<bool>,
    value: String,
    removing: bool,
    busy: bool,
    error: Option<String>,
}

thread_local! {
    static TOKENS: RefCell<[Token; 2]> = RefCell::new([Token::default(), Token::default()]);
}

fn token(at: usize, change: impl FnOnce(&mut Token)) {
    TOKENS.with(|tokens| change(&mut tokens.borrow_mut()[at]));
}

/// Saves what was typed, or removes the saved token. Main asks the host again with the new one.
fn change(at: usize, save: bool) {
    let value = TOKENS.with(|tokens| tokens.borrow()[at].value.trim().to_owned());
    if TOKENS.with(|tokens| tokens.borrow()[at].busy) {
        return;
    }
    token(at, |t| {
        t.busy = true;
        t.error = None;
    });
    spawn(async move {
        let done = if save { host::secret_set(HOSTS[at].secret, &value).await } else { host::secret_delete(HOSTS[at].secret).await };
        token(at, |t| {
            t.busy = false;
            match done {
                Ok(()) => {
                    t.saved = Some(save);
                    t.value.clear();
                    t.removing = false;
                }
                Err(error) => t.error = Some(error),
            }
        });
        if done_ok(at) {
            ipc::send(ipc::TOKENS_CHANGED, &Value::Null);
        }
    });
}

/// Whether the last change of this token worked: it has no error.
fn done_ok(at: usize) -> bool {
    TOKENS.with(|tokens| tokens.borrow()[at].error.is_none())
}

fn token_field(at: usize, t: &Token) -> Node {
    let host = &HOSTS[at];
    let state = match t.saved {
        None => "",
        Some(true) => "A token is saved.",
        Some(false) => "No token saved.",
    };
    let mut save = button("Save", "default", t.busy, move || change(at, true)).prop("size", "sm");
    if t.value.trim().is_empty() {
        save = save.prop("disabled", true);
    }
    let remove = (t.saved == Some(true)).then(|| button("Remove", "ghost", t.busy, move || token(at, |t| t.removing = true)).prop("size", "sm"));
    let input = text_input(&format!("{} token", host.name), &t.value, move |value| token(at, |t| t.value = value)).prop("type", "password").prop("autoComplete", "off");
    let confirm_remove = t.removing.then(|| {
        confirm(
            format!("Remove the {} token?", host.name),
            [text(format!("{} pull requests stop showing until you save a token again.", host.name)).prop("size", "small")],
            [button("Keep it", "outline", false, move || token(at, |t| t.removing = false)), button("Remove", "default", t.busy, move || change(at, false))],
        )
    });
    stack()
        .key(host.secret)
        .prop("gap", "s")
        .child(heading(3, host.name))
        .child(muted(host.help))
        .child(text(state))
        .child(row().prop("gap", "s").child(input).child(save).maybe(remove))
        .maybe(confirm_remove)
        .maybe(t.error.as_ref().map(|error| text(error.as_str()).prop("tone", "danger").prop("role", "alert")))
}

fn sidebar_label(value: &str) -> &'static str {
    if value == "all" { "Every open pull request" } else { "Yours and review requests" }
}

fn notify_label(value: &str) -> &'static str {
    match value {
        "review" => "Only when I'm asked to review",
        "all" => "Every new pull request",
        _ => "Off",
    }
}

fn sidebar_field() -> Node {
    let options: Vec<(String, String)> = SIDEBAR.iter().map(|s| ((*s).to_owned(), sidebar_label(s).to_owned())).collect();
    stack()
        .prop("gap", "s")
        .child(heading(1, "Sidebar"))
        .child(muted("The pull requests listed under each project. Yours come first, then the ones waiting for your review, then everyone else's."))
        .child(select("Sidebar lists", &options, prefs::sidebar_pulls().get(), |value| {
            if let Some(value) = SIDEBAR.iter().find(|s| **s == value) {
                prefs::sidebar_pulls().set(value).ok();
            }
        }))
}

fn watch_field() -> Node {
    let notify: Vec<(String, String)> = NOTIFY.iter().map(|n| ((*n).to_owned(), notify_label(n).to_owned())).collect();
    let minutes: Vec<(String, String)> = WATCH_MINUTES.iter().map(|m| (m.to_string(), format!("Every {m} minutes"))).collect();
    stack()
        .prop("gap", "s")
        .child(heading(1, "New pull requests"))
        .child(muted("Get a notification when someone opens a pull request in one of your projects, or only when one asks for your review, including one you're added to later. While Wings is in front it checks every minute. In the background it checks as often as you pick here."))
        .child(select("Notify me about", &notify, prefs::notify_pulls().get(), |value| {
            if let Some(value) = NOTIFY.iter().find(|n| **n == value) {
                prefs::notify_pulls().set(value).ok();
            }
        }))
        .child(select("Check in the background", &minutes, &prefs::watch_minutes().get().to_string(), |value| {
            prefs::watch_minutes().set(value.parse().ok().filter(|m| WATCH_MINUTES.contains(m)).unwrap_or(5)).ok();
        }))
}

fn flag_field(title: &str, help: &str, label: &str, on: bool, set: fn(bool)) -> Node {
    stack().prop("gap", "s").child(heading(1, title)).child(muted(help)).child(checkbox(label, on, set))
}

fn view() -> Vec<Node> {
    let tokens = TOKENS.with(|tokens| tokens.borrow().clone());
    vec![
        stack()
            .prop("gap", "l")
            .child(sidebar_field())
            .child(watch_field())
            .child(flag_field(
                "Comments for you",
                "Get a notification when someone mentions you, or replies in a thread you wrote in, on any pull request in your projects. It only checks while Wings is in the background, as often as above.",
                "Notify me about mentions and replies",
                prefs::mentions_on().get(),
                |on| drop(prefs::mentions_on().set(on)),
            ))
            .child(flag_field(
                "Agents",
                "When a coding agent works on a pull request's branch, send it a message when the pull request's checks fail, someone else comments on it or it gets merge conflicts. It checks those pull requests every 2 minutes, in the background too, and sends at most 5 messages an hour for each. Wings presses Enter for you only while \"Let agents message each other\" is on in Settings, Agents.",
                "Tell the agent on a PR's branch about failed checks and new review comments",
                prefs::tell_agents().get(),
                |on| drop(prefs::tell_agents().set(on)),
            ))
            .child(heading(1, "Pull request hosts"))
            .child(muted("GitHub (including GitHub Enterprise), GitLab and Azure DevOps use the gh, glab and az sign-in. Bitbucket Cloud and Codeberg use a token saved here."))
            .children(tokens.iter().enumerate().map(|(at, t)| token_field(at, t))),
    ]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    // The settings other frames change are read from storage on each build; a change made here is built again by the turn.
    for (at, host) in HOSTS.iter().enumerate() {
        spawn(async move {
            let saved = host::secret_has(host.secret).await.ok();
            token(at, |t| t.saved = saved);
        });
    }
}
