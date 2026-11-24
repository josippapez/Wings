use crate::{
    accounts::{Account, Loaded, Project, account_label, load, name_problem, sign_in_label, start_label},
    actions::{self, SignIn},
    state::{set_project, with},
    views::{form_problem, start_aria_label, start_title},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wings_plugin::{host::testing, task::block_on};

type Calls = Rc<RefCell<Vec<(String, Value)>>>;

/// Answers Wings' calls with made-up accounts and projects, and keeps each call. `fail` is a method that is refused.
fn wings(fail: Option<&'static str>) -> Calls {
    let calls: Calls = Rc::default();
    let seen = calls.clone();
    testing::answer(move |method, params| {
        seen.borrow_mut().push((method.to_owned(), params.clone()));
        if fail == Some(method) {
            return Err(format!("{method} refused"));
        }
        Ok(match method {
            "claudeAccounts" => json!({
                "accounts": [
                    { "name": null, "dir": "/home/ada/.claude", "email": "ada@example.com", "signedIn": true, "authMethod": "claude.ai", "sharesSetup": false },
                    { "name": "work", "dir": "/home/ada/.claude-work", "email": "ada@acme.example", "signedIn": true, "authMethod": "claude.ai", "sharesSetup": true },
                    { "name": "client-a", "dir": "/home/ada/.claude-client-a", "email": null, "signedIn": false, "authMethod": "none", "sharesSetup": true },
                ],
                "projects": { "/p/web": "work" },
            }),
            "projects" => json!([{ "id": "/p/web", "name": "web", "path": "/p/web" }, { "id": "/p/api", "name": "api", "path": "/p/api" }]),
            "openPane" => json!({ "paneId": "7" }),
            _ => Value::Null,
        })
    });
    calls
}

fn made(calls: &Calls, method: &str) -> Vec<Value> {
    calls.borrow().iter().filter(|(m, _)| m == method).map(|(_, p)| p.clone()).collect()
}

fn account(name: Option<&str>, email: Option<&str>, signed_in: bool) -> Account {
    Account { name: name.map(str::to_owned), dir: String::new(), email: email.map(str::to_owned), signed_in, shares_setup: false }
}

fn web() -> Project {
    Project { id: "/p/web".into(), name: "web".into(), path: "/p/web".into() }
}

fn loaded() -> Loaded {
    block_on(load()).unwrap()
}

#[test]
fn an_account_is_called_by_its_email_when_known_else_its_name() {
    let (default, named, bare) = (account(None, Some("ada@example.com"), true), account(Some("work"), Some("ada@acme.example"), true), account(Some("client-a"), None, true));
    assert_eq!(account_label(&default), "ada@example.com (default)");
    assert_eq!(account_label(&named), "ada@acme.example (work)");
    assert_eq!(account_label(&bare), "client-a");
    assert_eq!(account_label(&account(None, None, true)), "Default");
    assert_eq!(start_label(&named), "ada@acme.example");
    assert_eq!(start_label(&bare), "client-a");
    assert_eq!(start_label(&account(None, None, true)), "Default");
}

#[test]
fn the_list_says_who_is_signed_in_or_that_nobody_is() {
    assert_eq!(sign_in_label(&account(None, Some("ada@example.com"), true)), "ada@example.com");
    assert_eq!(sign_in_label(&account(None, None, true)), "Signed in");
    assert_eq!(sign_in_label(&account(Some("old"), Some("ada@example.com"), false)), "Not signed in (was ada@example.com)");
    assert_eq!(sign_in_label(&account(Some("new"), None, false)), "Not signed in");
}

#[test]
fn a_name_is_what_wings_accepts_and_not_taken() {
    let existing = [account(Some("work"), None, true)];
    assert_eq!(name_problem("client-a", &existing), None);
    assert_eq!(name_problem(&"a".repeat(32), &existing), None);
    assert_eq!(name_problem(&"a".repeat(33), &existing).as_deref(), Some("Use 32 characters or fewer."));
    assert_eq!(name_problem("Work", &existing).as_deref(), Some("Use only lowercase letters, digits and dashes."));
    assert_eq!(name_problem("my_work", &existing).as_deref(), Some("Use only lowercase letters, digits and dashes."));
    assert_eq!(name_problem("-work", &existing).as_deref(), Some("Start with a letter or a digit."));
    assert_eq!(name_problem("work", &existing).as_deref(), Some("There is an account called work already."));
}

#[test]
fn wrong_characters_show_as_typed_and_an_empty_name_waits_for_add() {
    assert_eq!(form_problem("", false, &[]), None);
    assert_eq!(form_problem("", true, &[]).as_deref(), Some("Enter a name for the account."));
    assert_eq!(form_problem("No", false, &[]).as_deref(), Some("Use only lowercase letters, digits and dashes."));
}

#[test]
fn the_accounts_and_the_projects_that_use_them_load_together() {
    let calls = wings(None);
    let data = loaded();
    assert_eq!(data.accounts.len(), 3);
    assert_eq!(data.account_of("/p/web"), Some("work"));
    assert_eq!(data.account_of("/p/api"), None);
    // The default account is the one a project that isn't listed uses.
    assert_eq!(data.projects_using(&data.accounts[0]), ["api"]);
    assert_eq!(data.projects_using(&data.accounts[1]), ["web"]);
    assert!(data.projects_using(&data.accounts[2]).is_empty());
    assert!(!data.accounts[2].signed_in && data.accounts[2].shares_setup);
    assert_eq!(made(&calls, "claudeAccounts").len(), 1);
}

#[test]
fn a_load_that_fails_says_why_and_keeps_what_it_had() {
    wings(Some("claudeAccounts"));
    with(|s| s.data = Some(Loaded::default()));
    block_on(actions::reload());
    with(|s| {
        assert_eq!(s.error.as_deref(), Some("claudeAccounts refused"));
        assert_eq!(s.data, Some(Loaded::default()));
    });
}

#[test]
fn picking_an_account_gives_it_to_the_project_and_says_so() {
    let calls = wings(None);
    block_on(actions::reload());
    block_on(actions::pick(web(), "~default".into()));
    // The default account goes as null.
    assert_eq!(made(&calls, "setClaudeAccount"), [json!({ "projectId": "/p/web", "name": null })]);
    // It loaded again after, and nothing started Claude.
    assert_eq!(made(&calls, "claudeAccounts").len(), 2);
    assert!(made(&calls, "openPane").is_empty());
    with(|s| {
        assert_eq!(s.note.as_ref().map(|n| (n.added, n.label.as_str())), Some((false, "ada@example.com (default)")));
        assert_eq!(s.picked, None);
    });
    block_on(actions::pick(web(), "client-a".into()));
    assert_eq!(made(&calls, "setClaudeAccount")[1], json!({ "projectId": "/p/web", "name": "client-a" }));
}

#[test]
fn picking_the_account_the_project_has_does_nothing() {
    let calls = wings(None);
    block_on(actions::reload());
    block_on(actions::pick(web(), "work".into()));
    assert!(made(&calls, "setClaudeAccount").is_empty());
    with(|s| assert_eq!(s.note, None));
}

#[test]
fn a_refused_switch_shows_the_error_and_no_message() {
    wings(None);
    block_on(actions::reload());
    let calls = wings(Some("setClaudeAccount"));
    block_on(actions::pick(web(), "client-a".into()));
    with(|s| {
        assert_eq!(s.action("swap").error.as_deref(), Some("setClaudeAccount refused"));
        assert_eq!((s.note.clone(), s.picked.clone()), (None, None));
    });
    // The list isn't loaded again for a change that didn't happen.
    assert_eq!(made(&calls, "claudeAccounts").len(), 0);
}

#[test]
fn open_claude_runs_it_in_the_project_under_the_projects_own_account() {
    let calls = wings(None);
    block_on(actions::open_claude(web()));
    // No `claudeAccount` at all: the project's own.
    assert_eq!(made(&calls, "openPane"), [json!({ "command": "claude", "args": [], "cwd": "/p/web", "placement": "tab" })]);
}

#[test]
fn start_claude_runs_it_as_another_account_and_leaves_the_project_alone() {
    let calls = wings(None);
    block_on(actions::start_claude(web(), Some("client-a".into())));
    block_on(actions::start_claude(web(), None));
    let panes = made(&calls, "openPane");
    assert_eq!(panes[0]["claudeAccount"], "client-a");
    // The default account is an explicit null.
    assert!(panes[1].as_object().unwrap().get("claudeAccount").is_some_and(Value::is_null));
    assert_eq!(panes[1]["command"], "claude");
    assert!(made(&calls, "setClaudeAccount").is_empty());
}

#[test]
fn use_my_default_setup_is_turned_on_and_off_by_name_and_the_list_loads_again() {
    let calls = wings(None);
    block_on(actions::share_setup("work".into(), false));
    assert_eq!(made(&calls, "shareClaudeSetup"), [json!({ "name": "work", "on": false })]);
    assert_eq!(made(&calls, "claudeAccounts").len(), 1);
    // The box stops showing the choice just made.
    with(|s| assert!(s.shares.is_empty()));
}

#[test]
fn a_refused_setup_change_says_why() {
    wings(Some("shareClaudeSetup"));
    block_on(actions::share_setup("work".into(), true));
    with(|s| assert_eq!(s.action("share:work").error.as_deref(), Some("shareClaudeSetup refused")));
}

#[test]
fn signing_in_opens_a_terminal_as_the_account_running_claude_auth_login() {
    let calls = wings(None);
    block_on(actions::sign_in("k".into(), Some("client-a".into()), "/p/web".into()));
    assert_eq!(
        made(&calls, "openPane"),
        [json!({ "command": "claude auth login", "args": [], "cwd": "/p/web", "placement": "tab", "claudeAccount": "client-a" })]
    );
}

#[test]
fn a_sign_in_is_over_when_claude_has_run_in_its_terminal_and_gone_or_the_terminal_closed() {
    let panes = |command: &str| vec![json!({ "paneId": "7", "command": command }), json!({ "paneId": "8", "command": "claude" })];
    let mut watch = SignIn { pane: "7".into(), ran: false, over: false };
    // The shell before claude starts, and claude running, aren't the end.
    assert!(!watch.hear(&panes("zsh")));
    assert!(!watch.hear(&panes("claude")));
    // Back to the shell: claude ended.
    assert!(watch.hear(&panes("zsh")));
    // It says so once.
    assert!(!watch.hear(&[]));

    let mut closed = SignIn { pane: "7".into(), ran: false, over: false };
    assert!(!closed.hear(&panes("zsh")));
    assert!(closed.hear(&[json!({ "paneId": "8", "command": "claude" })]));
}

#[test]
fn adding_an_account_names_it_for_the_project_and_opens_claude_there() {
    let calls = wings(None);
    block_on(actions::reload());
    with(|s| {
        s.adding = true;
        s.name = "client-b".into();
    });
    block_on(actions::add_account(web()));
    let order: Vec<String> = calls.borrow().iter().map(|(m, _)| m.clone()).filter(|m| m != "projects").collect();
    assert_eq!(order, ["claudeAccounts", "setClaudeAccount", "claudeAccounts", "openPane"]);
    assert_eq!(made(&calls, "setClaudeAccount"), [json!({ "projectId": "/p/web", "name": "client-b" })]);
    // `claude` opens under the new account, which is the project's now: no `claudeAccount` is sent.
    assert_eq!(made(&calls, "openPane"), [json!({ "command": "claude", "args": [], "cwd": "/p/web", "placement": "tab" })]);
    with(|s| {
        assert_eq!(s.note.as_ref().map(|n| (n.added, n.label.as_str())), Some((true, "client-b")));
        // The form is closed and empty.
        assert!((!s.adding, s.name.is_empty(), !s.submitted) == (true, true, true));
    });
}

#[test]
fn adding_an_account_with_no_name_or_a_bad_one_calls_nothing() {
    let calls = wings(None);
    block_on(actions::reload());
    for name in ["", "Work", "work"] {
        with(|s| s.name = name.into());
        block_on(actions::add_account(web()));
    }
    assert!(made(&calls, "setClaudeAccount").is_empty() && made(&calls, "openPane").is_empty());
    with(|s| assert!(s.submitted));
}

#[test]
fn another_project_starts_without_the_last_ones_message() {
    wings(None);
    set_project(&json!({ "id": "/p/web", "name": "web", "path": "/p/web" }));
    with(|s| s.note = Some(crate::state::Note { added: false, label: "work".into() }));
    // Renamed, still the same project: the message stays.
    set_project(&json!({ "id": "/p/web", "name": "website", "path": "/p/web" }));
    with(|s| assert!(s.note.is_some()));
    set_project(&json!({ "id": "/p/api", "name": "api", "path": "/p/api" }));
    with(|s| assert!(s.note.is_none()));
    // No project open is known, and is not the same as not knowing.
    set_project(&Value::Null);
    with(|s| assert_eq!(s.project, Some(None)));
}

#[test]
fn a_start_button_is_named_as_it_was_for_a_screen_reader() {
    assert_eq!(start_aria_label("ada@acme.example", false), "Start Claude as ada@acme.example");
    assert_eq!(start_aria_label("client-a", true), "Start Claude as client-a, not signed in");
}

#[test]
fn a_start_button_says_what_it_does_as_the_screen_before_the_port_did() {
    assert_eq!(start_title("ada@acme.example", false, true), "Start Claude as ada@acme.example in a new terminal. The project keeps the default account.");
    assert_eq!(start_title("client-a", true, false), "Start Claude as client-a in a new terminal. It is not signed in, so Claude won't use its plan. The project keeps its own account.");
}
