//! Signing in again from the chat. Claude Code runs here in stream-json mode, where its own `/login` can't run ("/login
//! isn't available in this environment"), so `/login` and the Sign in button on its "Not logged in" error run
//! `claude auth login` for the chat's account instead: Claude Code opens the browser itself and ends once you have
//! signed in. The chat then starts Claude Code again and sends the message that was refused.

use wings_plugin::{
    host, task,
    ui::{Node, button, muted, notice, row, stack},
};

use crate::{
    chat::{self, Chat},
    fork::Launch,
    session::Item,
};

/// How long the browser sign-in may take, the most Wings lets a program wait.
const WAIT_MS: u32 = 300_000;

/// A sign-in the chat started: running, or why the last one failed.
#[derive(Default)]
pub struct SignIn {
    pub busy: bool,
    pub error: Option<String>,
}

/// Whether a draft is Claude Code's `/login`, which the chat runs itself.
pub fn is_login(draft: &str) -> bool {
    draft.trim() == "/login"
}

/// Whether what Claude Code said is that nobody is signed in: "Not logged in · Please run /login", "Invalid API key ·
/// Please run /login".
pub fn needed(said: &str) -> bool {
    said.contains("Please run /login") || said.contains("Not logged in")
}

/// A line of the end of what `claude auth login` printed.
fn last_line(out: &serde_json::Value) -> Option<String> {
    ["stderr", "stdout"].iter().find_map(|key| out[key].as_str()?.lines().rev().find(|line| !line.trim().is_empty()).map(|line| line.trim().to_owned()))
}

/// Signs in to the chat's account in the browser, then goes on. One at a time.
pub fn start() {
    let started = chat::with(|c| {
        if c.signin.busy {
            return None;
        }
        c.signin = SignIn { busy: true, error: None };
        Some((chat::account(c, c.session.id.as_deref()), c.cwd.clone()))
    });
    let Some((account, cwd)) = started else { return };
    task::spawn(async move {
        let result = match host::exec_as(&account, &cwd, "claude", &["auth", "login"], Some(WAIT_MS)).await {
            Ok(out) if out["code"] == 0 => Ok(()),
            Ok(out) => Err(last_line(&out).unwrap_or_else(|| "Claude Code didn't finish signing in".into())),
            Err(error) => Err(error),
        };
        let signed_in = result.is_ok();
        chat::with(|c| c.signin = SignIn { busy: false, error: result.err() });
        if signed_in {
            go_on();
        }
    });
}

/// Starts Claude Code again in the tab, which now finds the sign-in. The session goes on when Claude Code has answered in
/// it (a refused first message leaves no transcript to resume), and the message it refused is sent again.
fn go_on() {
    let (resume, refused) = chat::with(|c| {
        let items = &c.session.items;
        let answered = items.iter().any(|item| !matches!(item, Item::User(_) | Item::UserImages(..) | Item::Error(_) | Item::Notice(_) | Item::Divider(_)));
        let refused = items.iter().rev().find(|item| !matches!(item, Item::Error(_) | Item::Notice(_))).and_then(|item| match item {
            Item::User(said) => Some(said.clone()),
            _ => None,
        });
        (c.session.id.clone().filter(|_| answered), refused)
    });
    chat::with(|c| c.first_prompt = refused);
    chat::launch_with(resume, Launch::default());
}

/// The Sign in button, which spins while the browser sign-in runs.
fn sign_in_button(c: &Chat) -> Node {
    row().child(button("Sign in", "default", c.signin.busy, start).prop("size", "sm"))
}

/// An error line of the conversation: with a Sign in button under it when it says nobody is signed in, and it is the last.
pub fn error_line(said: &str, last: bool, c: &Chat) -> Node {
    let line = notice("danger", said);
    if last && needed(said) { stack().prop("gap", "s").child(line).child(sign_in_button(c)) } else { line }
}

/// The banner of a Claude Code that stopped, with Sign in when it stopped for want of one.
pub fn failed_line(why: &str, c: &Chat) -> Node {
    error_line(why, true, c)
}

/// What the sign-in says while it runs, and why it failed.
pub fn status(c: &Chat) -> Option<Node> {
    if c.signin.busy {
        return Some(muted("Signing in. Finish in your browser, and this chat goes on by itself.").key("signin"));
    }
    c.signin.error.as_ref().map(|error| stack().prop("gap", "s").child(notice("danger", format!("Couldn't sign in: {error}"))).child(sign_in_button(c)).key("signin"))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::{Value, json};
    use wings_plugin::host;

    use super::*;
    use crate::chat::{State, with};

    type Calls = Rc<RefCell<Vec<(String, Value)>>>;

    /// Wings with a sign-in that ends with `code`, and each call it was asked.
    fn wings(code: i64) -> Calls {
        let calls = Calls::default();
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(match method {
                "exec" => json!({ "code": code, "stdout": "", "stderr": if code == 0 { "" } else { "Login timed out\n" } }),
                _ => Value::Null,
            })
        });
        calls
    }

    fn asked(calls: &Calls, method: &str) -> Vec<Value> {
        calls.borrow().iter().filter(|(called, _)| called == method).map(|(_, params)| params.clone()).collect()
    }

    /// The sign-ins asked for, and the starts of Claude Code (the fake one `refused` opens has no arguments).
    fn logins(calls: &Calls) -> Vec<Value> {
        asked(calls, "exec").into_iter().filter(|params| params["program"] == "claude").collect()
    }

    fn starts(calls: &Calls) -> Vec<Value> {
        asked(calls, "spawn").into_iter().filter(|params| params["args"].as_array().is_some_and(|args| !args.is_empty())).collect()
    }

    /// A chat for the account `work` that Claude Code refused the message "hi" in, for want of a sign-in.
    fn refused() {
        let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
        with(|c| {
            c.cwd = "/work/shop".into();
            c.picked = Some(Some("work".into()));
            c.state = State::Running;
            c.input = Some(process.input());
            c.session.items = vec![Item::User("hi".into()), Item::Error("Not logged in · Please run /login".into())];
        });
    }

    #[test]
    fn typing_login_signs_the_chats_account_in_and_is_not_sent_to_claude_code() {
        let calls = wings(0);
        refused();
        with(|c| c.draft = "/login".into());
        chat::send();
        wings_plugin::task::block_on(async {});
        let ran = logins(&calls);
        assert_eq!((ran.len(), ran[0]["program"].clone(), ran[0]["args"].clone()), (1, json!("claude"), json!(["auth", "login"])), "{ran:?}");
        assert_eq!(ran[0]["account"]["pick"]["name"], "work");
        assert_eq!(ran[0]["cwd"], "/work/shop");
        assert!(asked(&calls, "processWrite").is_empty(), "Claude Code would answer that /login isn't available here");
        assert!(with(|c| c.draft.is_empty()));
    }

    #[test]
    fn after_signing_in_claude_code_starts_again_and_gets_the_message_it_refused() {
        let calls = wings(0);
        refused();
        start();
        wings_plugin::task::block_on(async {});
        let started = starts(&calls);
        assert_eq!(started.len(), 1, "{started:?}");
        let args = started[0]["args"].to_string();
        assert!(!args.contains("--resume"), "a refused first message left no session to resume: {args}");
        let first: Value = serde_json::from_str(started[0]["input"].as_str().unwrap().lines().find(|line| line.contains("\"user\"")).unwrap()).unwrap();
        assert_eq!(first["message"]["content"], "hi");
        assert!(with(|c| !c.signin.busy && c.signin.error.is_none()));
    }

    #[test]
    fn a_sign_in_that_fails_says_why_offers_it_again_and_leaves_the_chat_as_it_was() {
        let calls = wings(1);
        refused();
        start();
        wings_plugin::task::block_on(async {});
        assert!(starts(&calls).is_empty());
        assert_eq!(with(|c| c.signin.error.clone()).as_deref(), Some("Login timed out"));
        let shown = with(|c| wings_plugin::ui::export(vec![status(c).unwrap()]).0.to_string());
        assert!(shown.contains("Couldn't sign in: Login timed out") && shown.contains("Sign in"), "{shown}");
    }

    #[test]
    fn the_not_logged_in_line_has_a_sign_in_button_that_signs_in_and_other_errors_have_none() {
        let calls = wings(0);
        refused();
        let (tree, handlers) = with(|c| wings_plugin::ui::export(vec![error_line("Not logged in · Please run /login", true, c)]));
        let button = tree[0]["children"][1]["children"][0].clone();
        assert_eq!(button["type"], "Button");
        assert!(handlers.call(button["props"]["onClick"].as_str().unwrap(), &json!({})));
        wings_plugin::task::block_on(async {});
        assert_eq!(logins(&calls).len(), 1);
        let plain = with(|c| wings_plugin::ui::export(vec![error_line("Overloaded", true, c)]).0.to_string());
        assert!(!plain.contains("Sign in"), "{plain}");
        let old = with(|c| wings_plugin::ui::export(vec![error_line("Not logged in · Please run /login", false, c)]).0.to_string());
        assert!(!old.contains("Sign in"), "an earlier error has no button: {old}");
    }
}
