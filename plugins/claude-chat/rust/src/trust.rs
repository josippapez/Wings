//! Workspace trust, asked as Claude Code's terminal and VS Code ask it. `claude -p` shows no trust dialog and runs a
//! folder's hooks, MCP servers and settings whether you trusted it or not, so the chat asks before it starts Claude Code
//! in a folder Claude Code holds no trust for (`claude workspace-trust --json`, which VS Code reads too). Trust starts it
//! with `initialize`'s `workspaceTrust`, which writes the trust in the same place the terminal's dialog does, so the
//! terminal and VS Code don't ask again. Don't trust starts it with `--ignore-persisted-trust`, as VS Code does for a
//! folder you didn't trust: the folder's hooks, MCP servers and settings stay unused. Claude Code keeps no record of a no,
//! so the tab keeps it, for its own starts, and a new chat there asks again, as a terminal does.

use serde_json::Value;
use wings_plugin::{
    host, task,
    ui::{Node, button, confirm, muted, row, text},
};

use crate::{
    chat::{self, Chat},
    fork::Launch,
    session::Item,
};

/// The flag that starts Claude Code without the folder's trust, which it reads as "set by an IDE host for a folder
/// nobody trusted".
pub const DECLINED: &str = "--ignore-persisted-trust";

/// How Claude Code is started in the folder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grant {
    /// As Claude Code has it: it trusts the folder already, or it couldn't say.
    Recorded,
    /// You trusted the folder: Claude Code records it as it starts.
    Trusted,
    /// You didn't: Claude Code starts without the folder's hooks, MCP servers and settings.
    Declined,
}

impl Grant {
    /// The flag Claude Code starts with.
    pub fn flag(self) -> Option<String> {
        (self == Grant::Declined).then(|| DECLINED.to_owned())
    }

    /// The folder `initialize` tells Claude Code you trusted: the one it runs in, whose trust it records under the
    /// repository's root, as the terminal's dialog does.
    pub fn trusted(self, cwd: &str) -> Option<&str> {
        (self == Grant::Trusted).then_some(cwd)
    }
}

/// A start of Claude Code that waits for your answer on the card: its number, the session it goes on with and how.
pub struct Held {
    run: u32,
    resume: Option<String>,
    how: Launch,
}

/// The card, when it shows.
#[derive(Default)]
pub enum Card {
    #[default]
    Closed,
    /// Before Claude Code starts in the folder.
    Start(Held),
    /// Over a Claude Code that runs without the folder's trust, from Review trust.
    Review,
}

/// What the tab knows of its folder's trust.
#[derive(Default)]
pub struct Trust {
    /// Your answer in this tab, which its next starts of Claude Code keep.
    pub answer: Option<bool>,
    /// What the trust covers, as Claude Code names it: the repository's root, or the folder outside one.
    folder: Option<String>,
    /// Whether that is a repository's root, which the trust covers whole.
    repository: bool,
    pub card: Card,
}

/// What `claude workspace-trust --json` printed, for a folder Claude Code holds no trust for: what the trust would
/// cover, and whether that is a repository. None for a trusted one, and when it couldn't say.
fn untrusted(out: &Result<Value, String>) -> Option<(String, bool)> {
    let out = out.as_ref().ok().filter(|out| out["code"] == 0)?;
    let said: Value = serde_json::from_str(out["stdout"].as_str()?.trim()).ok()?;
    if said["trusted"] != false {
        return None;
    }
    let root = said["root"].as_str();
    Some((root.or(said["directory"].as_str())?.to_owned(), root.is_some()))
}

/// How Claude Code `run` starts, as your answer in this tab or Claude Code's trust says. None while the card asks, whose
/// click starts it.
pub async fn settle(run: u32, resume: Option<&str>, how: &Launch) -> Option<Grant> {
    let (answer, account, cwd) = chat::with(|c| (c.trust.answer, chat::account(c, resume), c.cwd.clone()));
    match answer {
        Some(true) => return Some(Grant::Trusted),
        Some(false) => return Some(Grant::Declined),
        None => {}
    }
    // A Claude Code without the command, or one that can't read its config, says nothing: it starts as it did before.
    let out = host::exec_as(&account, &cwd, "claude", &["workspace-trust", "--json"], None).await;
    let Some((folder, repository)) = untrusted(&out) else {
        return Some(Grant::Recorded);
    };
    // A start made since then asks for itself.
    let asks = chat::with(|c| {
        if c.run != run {
            return false;
        }
        c.trust.folder = Some(folder);
        c.trust.repository = repository;
        c.trust.card = Card::Start(Held { run, resume: resume.map(str::to_owned), how: how.clone() });
        true
    });
    if asks {
        chat::report_agent();
    }
    None
}

/// Whether the card waits for you before Claude Code starts.
pub fn asks(c: &Chat) -> bool {
    matches!(c.trust.card, Card::Start(_))
}

/// The card: what trusting the folder lets run, Don't trust and Trust.
pub fn card(c: &Chat) -> Option<Node> {
    let review = match c.trust.card {
        Card::Closed => return None,
        Card::Start(_) => false,
        Card::Review => true,
    };
    let folder = c.trust.folder.as_deref().unwrap_or(&c.cwd);
    let covers = if c.trust.repository { "the whole repository" } else { "this folder" };
    let then = match review {
        true => "Trust starts Claude Code again on this session, with the folder's hooks, MCP servers and settings.",
        false => "Don't trust starts the chat without the folder's hooks, MCP servers and settings.",
    };
    let body = [
        text(folder).prop("mono", true).prop("size", "small"),
        text("Claude Code runs the hooks and MCP servers this folder sets up, and they can run any command on this Mac. Trust it only if it's your own code or comes from someone you trust."),
        muted(format!("Claude Code remembers a yes for {covers}, in the terminal and VS Code too. {then}")).prop("size", "small"),
    ];
    let actions = [button("Don't trust", "outline", false, || answer(false)), button("Trust", "default", false, || answer(true))];
    Some(confirm("Trust this folder?", body, actions).key("trust"))
}

/// Above the message box while Claude Code runs without the folder's trust: what is off, and Review trust, which brings
/// the card back.
pub fn declined_line(c: &Chat) -> Option<Node> {
    (c.trust.answer == Some(false) && matches!(c.trust.card, Card::Closed)).then(|| {
        row()
            .prop("gap", "s")
            .prop("align", "center")
            .prop("wrap", true)
            .child(muted("Running without this folder's hooks, MCP servers and settings.").prop("size", "small"))
            .child(button("Review trust", "ghost", false, || chat::with(|c| c.trust.card = Card::Review)).prop("size", "sm"))
            .key("trust-declined")
    })
}

/// What a click on the card does.
enum Next {
    Start(u32, Option<String>, Launch, Grant),
    Again(Option<String>),
    Nothing,
}

/// Your answer: the start the card held goes, with the folder's trust or without it. From Review trust, Trust starts
/// Claude Code again on the session, once Claude's turn is over, and Don't trust leaves it as it runs.
fn answer(yes: bool) {
    let next = chat::with(|c| {
        if matches!(c.trust.card, Card::Review) && yes {
            if let Some(why) = crate::fork_view::busy(c) {
                c.session.items.push(Item::Error(why.into()));
                return Next::Nothing;
            }
        }
        match std::mem::take(&mut c.trust.card) {
            Card::Start(held) if held.run == c.run => {
                c.trust.answer = Some(yes);
                Next::Start(held.run, held.resume, held.how, if yes { Grant::Trusted } else { Grant::Declined })
            }
            Card::Review if yes => {
                c.trust.answer = Some(true);
                // A session you haven't sent a message in has no transcript to go on with.
                Next::Again(c.session.id.clone().filter(|_| !c.session.prompts.is_empty()))
            }
            _ => Next::Nothing,
        }
    });
    match next {
        Next::Start(run, resume, how, grant) => task::spawn(chat::start_claude(run, resume, how, grant)),
        Next::Again(resume) => chat::launch_with(resume, Launch::default()),
        Next::Nothing => {}
    }
    chat::report_agent();
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::{Value, json};
    use wings_plugin::{host, ui};

    use super::*;
    use crate::chat::{State, with};

    type Calls = Rc<RefCell<Vec<(String, Value)>>>;

    /// What `claude workspace-trust --json` prints in `/work/shop/api`, a folder of the repository `/work/shop` that
    /// Claude Code holds no trust for.
    const UNTRUSTED: &str = r#"{"trusted":false,"directory":"/work/shop/api","root":"/work/shop","sandbox_limits_files":false,"may_renew_sign_in":true}"#;

    /// Wings, whose `claude workspace-trust --json` ends as `trust` says, keeping every call.
    fn wings(trust: Result<Value, String>) -> Calls {
        let calls = Calls::default();
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            match method {
                "exec" if params["args"][0] == "workspace-trust" => trust.clone(),
                _ => Ok(Value::Null),
            }
        });
        calls
    }

    fn printed(stdout: &str) -> Result<Value, String> {
        Ok(json!({ "code": 0, "stdout": format!("{stdout}\n"), "stderr": "" }))
    }

    fn asked(calls: &Calls, method: &str) -> Vec<Value> {
        calls.borrow().iter().filter(|(called, _)| called == method).map(|(_, params)| params.clone()).collect()
    }

    /// The reads of Claude Code's trust, among the commands the chat ran.
    fn trust_reads(calls: &Calls) -> Vec<Value> {
        asked(calls, "exec").into_iter().filter(|params| params["args"][0] == "workspace-trust").collect()
    }

    /// A chat in `/work/shop/api` starting Claude Code anew.
    fn open() {
        with(|c| c.cwd = "/work/shop/api".into());
        chat::launch_with(None, Launch::default());
        task::block_on(async {});
    }

    /// The `initialize` a start of Claude Code sends first.
    fn initialize(spawned: &Value) -> Value {
        serde_json::from_str(spawned["input"].as_str().unwrap().lines().next().unwrap()).unwrap()
    }

    fn flagged(spawned: &Value) -> bool {
        spawned["args"].as_array().unwrap().contains(&json!(DECLINED))
    }

    /// Clicks the button that reads `label` among `nodes`.
    fn press(nodes: Vec<Node>, label: &str) {
        fn find<'a>(node: &'a Value, label: &str) -> Option<&'a Value> {
            let named = node["type"] == "Button" && node["children"].as_array().is_some_and(|children| children.iter().any(|child| child["text"] == label));
            if named {
                return Some(node);
            }
            node["children"].as_array()?.iter().find_map(|child| find(child, label))
        }
        let (tree, handlers) = ui::export(nodes);
        let button = tree.as_array().unwrap().iter().find_map(|node| find(node, label)).unwrap_or_else(|| panic!("no {label} in {tree}"));
        assert!(handlers.call(button["props"]["onClick"].as_str().unwrap(), &json!({})));
        task::block_on(async {});
    }

    fn the_card() -> Vec<Node> {
        with(|c| card(c).into_iter().collect())
    }

    #[test]
    fn a_folder_claude_code_does_not_trust_asks_first_and_starts_nothing_until_you_answer() {
        let calls = wings(printed(UNTRUSTED));
        open();
        let read = trust_reads(&calls);
        assert_eq!((read.len(), &read[0]["program"], &read[0]["args"], &read[0]["cwd"]), (1, &json!("claude"), &json!(["workspace-trust", "--json"]), &json!("/work/shop/api")));
        assert!(asked(&calls, "spawn").is_empty(), "the folder's hooks and MCP servers would run as Claude Code starts");
        let shown = ui::export(the_card()).0.to_string();
        assert!(shown.contains("Trust this folder?") && shown.contains("/work/shop") && shown.contains("the whole repository"), "{shown}");
        assert_eq!(asked(&calls, "setAgent").last().unwrap()["agent"]["state"], "blocked", "the Agents list says the chat waits on you");
    }

    #[test]
    fn trust_starts_claude_code_with_the_trust_it_records_as_its_own_dialog_does() {
        let calls = wings(printed(UNTRUSTED));
        open();
        press(the_card(), "Trust");
        let spawned = asked(&calls, "spawn");
        assert_eq!(spawned.len(), 1);
        assert!(!flagged(&spawned[0]));
        let first = initialize(&spawned[0]);
        assert_eq!(first["request"]["subtype"], "initialize");
        assert_eq!(first["request"]["workspaceTrust"], json!({ "accepted": true, "directory": "/work/shop/api" }));
        assert!(with(|c| card(c).is_none() && declined_line(c).is_none()));
    }

    #[test]
    fn dont_trust_starts_it_without_the_folders_hooks_and_settings_and_the_tab_keeps_that() {
        let calls = wings(printed(UNTRUSTED));
        open();
        press(the_card(), "Don't trust");
        let spawned = asked(&calls, "spawn");
        assert_eq!(spawned.len(), 1);
        assert!(flagged(&spawned[0]), "{}", spawned[0]["args"]);
        assert!(initialize(&spawned[0])["request"].get("workspaceTrust").is_none());
        with(|c| c.state = State::Running);
        let line = ui::export(with(|c| declined_line(c).into_iter().collect())).0.to_string();
        assert!(line.contains("Running without this folder's hooks, MCP servers and settings") && line.contains("Review trust"), "{line}");
        // The next start in the tab, as a new chat here, goes on without asking.
        open();
        assert_eq!(trust_reads(&calls).len(), 1, "asked once");
        let spawned = asked(&calls, "spawn");
        assert_eq!(spawned.len(), 2);
        assert!(flagged(&spawned[1]) && with(|c| card(c).is_none()));
    }

    #[test]
    fn a_folder_claude_code_trusts_starts_at_once_as_before() {
        let calls = wings(printed(r#"{"trusted":true,"directory":"/work/shop/api","root":"/work/shop"}"#));
        open();
        let spawned = asked(&calls, "spawn");
        assert_eq!(spawned.len(), 1);
        assert!(!flagged(&spawned[0]) && initialize(&spawned[0])["request"].get("workspaceTrust").is_none());
        assert!(with(|c| card(c).is_none()));
    }

    #[test]
    fn a_claude_code_that_cannot_say_starts_as_before() {
        for said in [Err("claude-chat may not run claude workspace-trust".to_owned()), Ok(json!({ "code": 1, "stdout": "", "stderr": "error: unknown command" })), printed("not json")] {
            let calls = wings(said);
            open();
            let spawned = asked(&calls, "spawn");
            assert_eq!(spawned.len(), 1);
            assert!(!flagged(&spawned[0]) && with(|c| card(c).is_none()));
        }
    }

    #[test]
    fn review_trust_brings_the_card_back_and_trust_starts_claude_code_again_on_the_session_once_the_turn_is_over() {
        let calls = wings(printed(UNTRUSTED));
        open();
        press(the_card(), "Don't trust");
        with(|c| {
            c.state = State::Running;
            c.session.id = Some("s1".into());
            c.session.prompts.push(crate::session::Prompt { at: 0, uuid: "u1".into(), parent: None });
            c.session.working = true;
        });
        press(with(|c| declined_line(c).into_iter().collect()), "Review trust");
        assert!(with(|c| declined_line(c).is_none()));
        // Mid-turn, it says why and stays.
        press(the_card(), "Trust");
        assert_eq!(asked(&calls, "spawn").len(), 1);
        assert!(with(|c| matches!(c.session.items.last(), Some(Item::Error(why)) if why.contains("Claude is working"))));
        with(|c| c.session.working = false);
        press(the_card(), "Trust");
        let spawned = asked(&calls, "spawn");
        assert_eq!(spawned.len(), 2);
        assert!(spawned[1]["args"].as_array().unwrap().contains(&json!("--resume=s1")) && !flagged(&spawned[1]));
        assert_eq!(initialize(&spawned[1])["request"]["workspaceTrust"]["accepted"], true);
    }

    #[test]
    fn dont_trust_from_review_leaves_claude_code_running_as_it_was() {
        let calls = wings(printed(UNTRUSTED));
        open();
        press(the_card(), "Don't trust");
        with(|c| c.state = State::Running);
        press(with(|c| declined_line(c).into_iter().collect()), "Review trust");
        press(the_card(), "Don't trust");
        assert_eq!(asked(&calls, "spawn").len(), 1);
        assert!(with(|c| card(c).is_none() && declined_line(c).is_some()));
    }
}
