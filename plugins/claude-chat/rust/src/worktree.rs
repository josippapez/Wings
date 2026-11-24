//! The Worktree button in the message box, as in Claude's desktop app: before the first message, turn it on and the chat
//! runs in a git worktree of its project instead of the project's folder. Wings makes it (`createWorktree`), so it is
//! listed under the project like the ones Wings makes itself; this module starts Claude Code there and then sends the
//! message. A running session can't move, so the button is gone once there is a message. The choice is this chat's only.

use std::cell::RefCell;

use serde_json::Value;
use wings_plugin::{
    format::basename,
    host, task,
    ui::{Node, button, icon, text},
};

use crate::{
    chat::{self, State},
    fork::Launch,
    session::Item,
};

/// Lucide's `git-fork`, in a 24-point box.
const FORK: [&str; 5] = [
    "M9 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    "M3 6a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    "M15 6a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    "M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9",
    "M12 12v3",
];

/// The words of the message the name is made of, and the most of it kept.
const WORDS: usize = 5;
const SLUG_LEN: usize = 40;

/// How many names are tried when the one made from the message is taken: `-2`, `-3` ...
const TRIES: u32 = 5;

#[derive(Default)]
struct Worktree {
    /// The button is on.
    on: bool,
    /// A worktree is being made, so a second Enter doesn't make another.
    busy: bool,
    /// The start of Claude Code that sends the message once it runs, which is the one after the worktree was made.
    pending: Option<u32>,
    /// The folder the chat moved to, which the tab keeps: a session is found by its folder, so a chat that opens again
    /// has to start there.
    made: Option<String>,
}

thread_local! {
    static WORKTREE: RefCell<Worktree> = RefCell::new(Worktree::default());
}

fn with<T>(change: impl FnOnce(&mut Worktree) -> T) -> T {
    WORKTREE.with(|state| change(&mut state.borrow_mut()))
}

/// The branch for a worktree made for `message`: its first words, as `claude/fix-the-login-bug`, and from the second
/// `attempt` on a number after them. Only lower case letters, digits and dashes, so git always takes it.
pub fn branch_name(message: &str, attempt: u32) -> String {
    let mut slug = String::new();
    for word in message.split_whitespace().take(WORDS) {
        let word: String = word.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' }).collect();
        for part in word.split('-').filter(|part| !part.is_empty()) {
            if !slug.is_empty() {
                slug.push('-');
            }
            slug.push_str(part);
        }
    }
    slug.truncate(SLUG_LEN);
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "chat" } else { slug };
    if attempt > 1 { format!("claude/{slug}-{attempt}") } else { format!("claude/{slug}") }
}

/// Whether the button shows: the chat is in a git folder and nothing is said in it yet. After a message the session is
/// in the folder it started in, and a session found again later is in that one.
pub fn offered(c: &chat::Chat) -> bool {
    !c.cwd.is_empty()
        && c.branch.is_some()
        && matches!(c.state, State::Starting | State::Running)
        && !c.session.items.iter().any(|item| matches!(item, Item::User(_) | Item::UserImages(..)))
}

/// The folder a chat that was moved into a worktree starts in when it opens again, for the tab to keep.
pub fn made() -> Option<String> {
    with(|state| state.made.clone())
}

/// A chat that opens again in the worktree it was moved to.
pub fn restore(cwd: Option<&str>) {
    with(|state| state.made = cwd.map(str::to_owned));
}

/// The button turns the worktree on or off.
pub fn toggle() {
    if chat::with(|c| offered(c)) && !with(|state| state.busy) {
        with(|state| state.on = !state.on);
    }
}

/// The button for the message box's row, while it is offered. A narrow tab keeps the icon.
pub fn button_node(c: &chat::Chat) -> Option<Node> {
    if !offered(c) {
        return None;
    }
    let (on, busy) = with(|state| (state.on, state.busy));
    let title = if on { "Worktree is on: the first message starts this chat in a new git worktree. Click to turn it off" } else { "Worktree: start this chat in its own git worktree, an isolated copy of the project" };
    let glyph = icon(24, 2.0, &FORK).prop("size", 14);
    let variant = if on { "secondary" } else { "ghost" };
    let node = if c.narrow == Some(true) {
        button("", variant, false, toggle).prop("size", "icon-sm").prop("round", true).child(glyph)
    } else {
        button("", variant, false, toggle).prop("size", "sm").child(glyph).child(text("Worktree"))
    };
    Some(node.prop("label", if on { "Worktree, on" } else { "Worktree" }).prop("title", title).prop("disabled", busy).key("worktree"))
}

/// Takes the send of the first message when the button is on: makes the worktree, starts Claude Code in it, and sends the
/// message from there. Says whether it took it. A command (`/…`) goes to the folder the chat is in.
pub fn intercept() -> bool {
    let taken = chat::with(|c| {
        let text = c.draft.trim();
        if text.is_empty() && c.images.is_empty() || text.starts_with('/') || !offered(c) {
            return None;
        }
        if !with(|state| state.on && !std::mem::replace(&mut state.busy, true)) {
            return None;
        }
        Some((c.run, c.cwd.clone(), c.draft.clone()))
    });
    let Some((run, cwd, message)) = taken else {
        return false;
    };
    task::spawn(async move {
        let made = make(&cwd, &message).await;
        with(|state| state.busy = false);
        // The chat was started over while the worktree was made: the message is not for it any more.
        if chat::with(|c| c.run) != run {
            return;
        }
        match made {
            // The message stays in the box, with what went wrong above it.
            Err(error) => chat::with(|c| c.session.items.push(Item::Error(format!("Couldn't make a worktree: {error}")))),
            Ok(path) => {
                let next = chat::with(|c| {
                    c.folder = basename(&path).to_owned();
                    c.cwd = path.clone();
                    c.branch = None;
                    c.run + 1
                });
                with(|state| {
                    state.on = false;
                    state.made = Some(path);
                    state.pending = Some(next);
                });
                chat::launch_with(None, Launch::default());
            }
        }
    });
    true
}

/// Claude Code of start `run` runs: the first message goes now when it is the one started in the new worktree.
pub fn started(run: u32) {
    let mine = with(|state| {
        let mine = state.pending == Some(run);
        if mine {
            state.pending = None;
        }
        mine
    });
    if mine {
        chat::send();
    }
}

/// A worktree of the chat's project, as Wings' `createWorktree` makes it, on a branch named from `message`: its folder.
async fn make(cwd: &str, message: &str) -> Result<String, String> {
    // The project the folder is: a worktree's own project is its repo's, so the new one sits beside the same repo.
    let project = host::project().await.unwrap_or(Value::Null);
    let id = match (project["path"].as_str() == Some(cwd), project["parentId"].as_str(), project["id"].as_str()) {
        (true, Some(parent), _) => parent.to_owned(),
        (true, None, Some(id)) => id.to_owned(),
        _ => cwd.to_owned(),
    };
    let mut attempt = 1;
    loop {
        match host::create_worktree(Some(&id), &branch_name(message, attempt), None).await {
            Ok(made) => return made["path"].as_str().map(str::to_owned).ok_or_else(|| "Wings didn't say where it made it".to_owned()),
            // A name that is taken (the branch or its folder): the next one.
            Err(error) if error.contains("already exists") && attempt < TRIES => attempt += 1,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    type Calls = Rc<RefCell<Vec<(String, Value)>>>;

    /// Wings, as the chat asks it: every call kept, `createWorktree` answered by `made`.
    fn wings(project: Value, made: impl Fn(&Value) -> Result<Value, String> + 'static) -> Calls {
        let calls = Calls::default();
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            match method {
                "project" => Ok(project.clone()),
                "createWorktree" => made(params),
                _ => Ok(Value::Null),
            }
        });
        calls
    }

    fn called(calls: &Calls, method: &str) -> Vec<Value> {
        calls.borrow().iter().filter(|(name, _)| name == method).map(|(_, params)| params.clone()).collect()
    }

    fn ready(draft: &str) {
        with(|state| *state = Worktree::default());
        chat::with(|c| {
            c.cwd = "/work/shop".into();
            c.folder = "shop".into();
            c.branch = Some("main".into());
            c.state = State::Running;
            c.session.items.clear();
            c.draft = draft.into();
            c.run += 1;
        });
    }

    fn drawn() -> Option<String> {
        chat::with(|c| button_node(c)).map(|node| wings_plugin::ui::tree_json(node).to_string())
    }

    #[test]
    fn the_branch_is_made_of_the_first_words_in_a_form_git_takes_and_gets_a_number_when_taken() {
        assert_eq!(branch_name("Fix the login bug on the settings page", 1), "claude/fix-the-login-bug-on");
        assert_eq!(branch_name("Fix the login bug", 2), "claude/fix-the-login-bug-2");
        assert_eq!(branch_name("  Add `foo::bar()` -- now!!  ", 1), "claude/add-foo-bar-now");
        assert_eq!(branch_name("é ü ?! ...", 1), "claude/chat");
        assert_eq!(branch_name("", 3), "claude/chat-3");
        let long = branch_name(&"abcdefghij ".repeat(10), 1);
        assert!(long.len() <= "claude/".len() + SLUG_LEN && !long.ends_with('-'), "{long}");
        // A slash, a dot pair or a leading dash in the message never reaches git.
        assert_eq!(branch_name("../../etc -rf a/b", 1), "claude/etc-rf-a-b");
    }

    #[test]
    fn the_button_shows_only_in_a_git_folder_before_the_first_message() {
        ready("");
        assert!(drawn().unwrap().contains("Worktree"));
        chat::with(|c| c.branch = None);
        assert!(drawn().is_none(), "not a git folder");
        chat::with(|c| c.branch = Some("main".into()));
        chat::with(|c| c.session.items.push(Item::Text("Hello".into())));
        assert!(drawn().is_some(), "something Claude said is not a message of yours");
        chat::with(|c| c.session.items.push(Item::User("hi".into())));
        assert!(drawn().is_none(), "gone after the first message");
        chat::with(|c| c.session.items = vec![Item::UserImages(String::new(), vec!["a".into()])]);
        assert!(drawn().is_none());
        chat::with(|c| {
            c.session.items.clear();
            c.state = State::Failed("no".into());
        });
        assert!(drawn().is_none());
    }

    #[test]
    fn the_button_is_lit_while_on_and_a_narrow_tab_keeps_the_icon() {
        ready("");
        let off = drawn().unwrap();
        assert_eq!(off.matches("\"Worktree\"").count(), 2, "the label and the text: {off}");
        assert!(off.contains("\"ghost\"") && off.contains("Worktree: start this chat"), "{off}");
        toggle();
        let on = drawn().unwrap();
        assert!(on.contains("\"secondary\"") && on.contains("Worktree is on"), "{on}");
        toggle();
        assert!(drawn().unwrap().contains("\"ghost\""));
        chat::with(|c| c.narrow = Some(true));
        assert_eq!(drawn().unwrap().matches("\"Worktree\"").count(), 1, "only the label is left: {}", drawn().unwrap());
        chat::with(|c| c.narrow = None);
        // Once there is a message, it can't be turned on.
        chat::with(|c| c.session.items.push(Item::User("hi".into())));
        toggle();
        assert!(!with(|state| state.on));
    }

    #[test]
    fn with_the_button_off_or_a_command_typed_the_send_is_left_alone() {
        let calls = wings(json!(null), |_| Ok(json!({})));
        ready("fix the build");
        assert!(!intercept());
        toggle();
        chat::with(|c| c.draft = "/usage".into());
        assert!(!intercept());
        chat::with(|c| c.draft = "   ".into());
        assert!(!intercept());
        task::block_on(async {});
        assert!(called(&calls, "createWorktree").is_empty());
    }

    #[test]
    fn the_first_message_makes_the_worktree_starts_claude_code_there_and_is_sent_from_it() {
        let calls = wings(json!({ "id": "/work/shop", "name": "shop", "path": "/work/shop" }), |params| {
            let branch = params["branch"].as_str().unwrap();
            Ok(json!({ "projectId": "/work/shop-worktrees/x", "path": format!("/work/shop-worktrees/{}", branch.replace('/', "-")) }))
        });
        ready("Fix the login bug");
        toggle();
        assert!(intercept());
        // A second Enter meanwhile makes no second worktree.
        assert!(!intercept());
        task::block_on(async {});
        let made = called(&calls, "createWorktree");
        assert_eq!(made, [json!({ "projectId": "/work/shop", "branch": "claude/fix-the-login-bug", "base": null })]);
        // Claude Code starts in the new folder, once, and the header names it.
        let spawned = called(&calls, "spawn");
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0]["cwd"], "/work/shop-worktrees/claude-fix-the-login-bug");
        assert_eq!(chat::with(|c| (c.cwd.clone(), c.folder.clone())), ("/work/shop-worktrees/claude-fix-the-login-bug".to_owned(), "claude-fix-the-login-bug".to_owned()));
        // The message went out from there, as yours.
        let wrote = called(&calls, "processWrite");
        assert!(wrote.iter().any(|write| write["text"].as_str().is_some_and(|text| text.contains("Fix the login bug"))), "{wrote:?}");
        assert!(chat::with(|c| c.session.items.iter().any(|item| matches!(item, Item::User(said) if said == "Fix the login bug"))));
        // The button is spent: off, and gone with the message. The tab keeps the folder.
        assert!(!with(|state| state.on) && drawn().is_none());
        assert_eq!(made_folder(), Some("/work/shop-worktrees/claude-fix-the-login-bug".to_owned()));
    }

    fn made_folder() -> Option<String> {
        super::made()
    }

    #[test]
    fn a_chat_in_a_worktree_makes_the_new_one_of_its_repo() {
        let calls = wings(json!({ "id": "/work/shop-worktrees/a", "path": "/work/shop", "parentId": "/work/repo" }), |_| Ok(json!({ "path": "/w/x" })));
        ready("go");
        toggle();
        intercept();
        task::block_on(async {});
        assert_eq!(called(&calls, "createWorktree")[0]["projectId"], "/work/repo");
        // A folder that is not the project's own: the folder is asked for.
        let calls = wings(json!({ "id": "/work/other-id", "path": "/work/other" }), |_| Ok(json!({ "path": "/w/x" })));
        ready("go");
        toggle();
        intercept();
        task::block_on(async {});
        assert_eq!(called(&calls, "createWorktree")[0]["projectId"], "/work/shop");
    }

    #[test]
    fn a_taken_name_gets_the_next_one() {
        let taken = Rc::new(RefCell::new(2));
        let left = taken.clone();
        let calls = wings(json!(null), move |params| {
            if *left.borrow() > 0 {
                *left.borrow_mut() -= 1;
                return Err(format!("{} already exists", params["branch"]));
            }
            Ok(json!({ "path": "/w/made" }))
        });
        ready("Fix the login bug");
        toggle();
        intercept();
        task::block_on(async {});
        let branches: Vec<_> = called(&calls, "createWorktree").iter().map(|params| params["branch"].as_str().unwrap().to_owned()).collect();
        assert_eq!(branches, ["claude/fix-the-login-bug", "claude/fix-the-login-bug-2", "claude/fix-the-login-bug-3"]);
        assert_eq!(called(&calls, "spawn")[0]["cwd"], "/w/made");
    }

    #[test]
    fn when_wings_refuses_the_message_stays_in_the_box_and_the_chat_stays_in_its_folder() {
        for why in ["/work/shop isn't a git repository", "No such project in Wings", "worktrees isn't allowed for this plugin"] {
            let calls = wings(json!(null), move |_| Err(why.to_owned()));
            ready("Fix the login bug");
            toggle();
            assert!(intercept());
            task::block_on(async {});
            assert!(called(&calls, "spawn").is_empty(), "{why}");
            let said: Vec<String> = chat::with(|c| c.session.items.iter().filter_map(|item| if let Item::Error(said) = item { Some(said.clone()) } else { None }).collect());
            assert_eq!(said, [format!("Couldn't make a worktree: {why}")]);
            assert_eq!(chat::with(|c| (c.draft.clone(), c.cwd.clone())), ("Fix the login bug".to_owned(), "/work/shop".to_owned()));
            assert!(made_folder().is_none());
            // The button stays on and free, to try again.
            assert!(with(|state| state.on && !state.busy));
        }
        // A name taken every time is given up on, with git's words.
        let calls = wings(json!(null), |_| Err("A branch named x already exists".into()));
        ready("Fix");
        toggle();
        intercept();
        task::block_on(async {});
        assert_eq!(called(&calls, "createWorktree").len(), TRIES as usize);
        assert!(called(&calls, "spawn").is_empty());
    }
}
