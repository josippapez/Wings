//! The branch button in the message box: the branch the chat's folder is on, and a list to find another and switch to it
//! with `git switch`. It is off while Claude works on a turn, and a switch git refuses (changes in the way) shows git's
//! own words in the conversation: nothing is forced or stashed.

use std::cell::RefCell;

use serde_json::Value;
use wings_plugin::{
    format::truncate,
    git, host, task,
    ui::{Choice, Combobox, Node, button, card, combobox, icon, loading, muted, row, stack, text},
};

use crate::{chat, session::Item};

/// Lucide's `git-branch`, in a 24-point box.
const BRANCH: [&str; 4] = ["M6 3v12", "M15 6a3 3 0 1 0 6 0a3 3 0 1 0-6 0", "M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0", "M18 9a9 9 0 0 1-9 9"];
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";

/// The branches the list shows at once: a repo can have hundreds, and typing narrows them.
const SHOWN: usize = 12;

/// A branch to switch to: a local one, or one that is only on a remote, which `git switch` makes a local one that follows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    pub name: String,
    pub remote_only: bool,
}

#[derive(Default)]
struct Switcher {
    open: bool,
    /// The search since the field took the keyboard, as `ui::Combobox` keeps it for a picker.
    query: Option<Option<String>>,
    /// The branches read when the list opened, none until git answers.
    branches: Option<Vec<Branch>>,
}

thread_local! {
    static SWITCHER: RefCell<Switcher> = RefCell::new(Switcher::default());
}

fn with<T>(change: impl FnOnce(&mut Switcher) -> T) -> T {
    SWITCHER.with(|switcher| change(&mut switcher.borrow_mut()))
}

/// The branches in `git for-each-ref --format=%(refname) refs/heads refs/remotes`, but `current`: local ones first, then
/// those only on a remote (`origin/x` as `x`, which is what `git switch x` takes), each once, in git's order.
pub fn parse_branches(refs: &str, current: &str) -> Vec<Branch> {
    let mut local: Vec<String> = Vec::new();
    let mut remote: Vec<String> = Vec::new();
    for line in refs.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            local.push(name.to_owned());
        } else if let Some((_, name)) = line.strip_prefix("refs/remotes/").and_then(|rest| rest.split_once('/')) {
            // `origin/HEAD` is a pointer, not a branch.
            if name != "HEAD" && !remote.iter().any(|seen| seen == name) {
                remote.push(name.to_owned());
            }
        }
    }
    let local_names = local.clone();
    local
        .into_iter()
        .map(|name| Branch { name, remote_only: false })
        .chain(remote.into_iter().filter(|name| !local_names.contains(name)).map(|name| Branch { name, remote_only: true }))
        .filter(|branch| branch.name != current)
        .collect()
}

/// The branches whose name holds `query` (case does not matter), at most `limit`, and how many matched in all.
pub fn matching<'a>(branches: &'a [Branch], query: &str, limit: usize) -> (Vec<&'a Branch>, usize) {
    let wanted = query.trim().to_lowercase();
    let found: Vec<&Branch> = branches.iter().filter(|branch| branch.name.to_lowercase().contains(&wanted)).collect();
    let total = found.len();
    (found.into_iter().take(limit).collect(), total)
}

/// What `git switch` answered: done, or what it said, which for changes that would be overwritten names the files.
pub fn outcome(answer: Result<Value, String>) -> Result<(), String> {
    let answer = answer?;
    if answer["code"] == 0 {
        return Ok(());
    }
    let said = |field: &str| answer[field].as_str().map(str::trim).filter(|said| !said.is_empty()).map(str::to_owned);
    Err(said("stderr").or_else(|| said("stdout")).unwrap_or_else(|| "git switch failed".to_owned()))
}

async fn list(cwd: &str, current: &str) -> Vec<Branch> {
    git::ok(cwd, &["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]).await.map_or_else(Vec::new, |refs| parse_branches(&refs, current))
}

async fn switch(cwd: &str, name: &str) -> Result<(), String> {
    outcome(host::exec_in(cwd, "git", &["switch", name]).await)
}

/// The button opens the list, and closes it again. Reading the branches starts each time it opens, so a branch made
/// meanwhile is there.
pub fn toggle() {
    let (cwd, run, current, working) = chat::with(|c| (c.cwd.clone(), c.run, c.branch.clone().unwrap_or_default(), c.session.working));
    if working {
        return;
    }
    let open = with(|s| {
        s.open = !s.open;
        s.query = None;
        s.branches = None;
        s.open
    });
    if !open {
        return;
    }
    task::spawn(async move {
        let branches = list(&cwd, &current).await;
        if chat::with(|c| c.run) == run {
            with(|s| {
                if s.open {
                    s.branches = Some(branches);
                }
            });
        }
    });
}

pub fn close() {
    with(|s| *s = Switcher::default());
}

fn searched(text: Option<String>) {
    with(|s| s.query = Some(text));
}

/// Switches to `name`, then has the chat read its branch again for the header. A refusal is a line in the conversation.
pub fn pick(name: &str) {
    let (cwd, run, working) = chat::with(|c| (c.cwd.clone(), c.run, c.session.working));
    if working {
        return;
    }
    close();
    let name = name.to_owned();
    task::spawn(async move {
        match switch(&cwd, &name).await {
            Ok(()) => chat::load_git(run, cwd),
            Err(error) => chat::with(|c| {
                if c.run == run {
                    c.session.items.push(Item::Error(format!("Couldn't switch to {name}: {error}")));
                }
            }),
        }
    });
}

/// The button for the message box's row: the branch the chat is on. Only where the folder is on a branch. A narrow tab
/// keeps the icon, and the title says the rest.
pub fn button_node(c: &chat::Chat) -> Option<Node> {
    let name = c.branch.as_deref()?;
    let working = c.session.working;
    let open = with(|s| s.open) && !working;
    let title = if working { "Claude is working on a turn: switch branch when it is done".to_owned() } else { format!("Branch {name}. Switch branch") };
    let glyph = icon(24, 2.0, &BRANCH).prop("size", 14);
    let variant = if open { "secondary" } else { "ghost" };
    let node = if c.narrow == Some(true) {
        button("", variant, false, toggle).prop("size", "icon-sm").prop("round", true).child(glyph)
    } else {
        button("", variant, false, toggle).prop("size", "sm").child(glyph).child(text(truncate(name, 24)).prop("truncate", true))
    };
    Some(node.prop("label", "Switch branch").prop("title", title.as_str()).prop("disabled", working).key("branch"))
}

/// The list over the message box while it is open: a search field and the branches that match, which a click switches to.
pub fn panel(c: &chat::Chat) -> Option<Node> {
    if c.session.working || c.branch.is_none() {
        return None;
    }
    let (open, query, branches) = with(|s| (s.open, s.query.clone().flatten(), s.branches.clone()));
    if !open {
        return None;
    }
    let body = match branches {
        None => loading(),
        Some(branches) if branches.is_empty() => muted("There is no other branch in this folder.").prop("size", "small"),
        Some(branches) => {
            let (shown, total) = matching(&branches, query.as_deref().unwrap_or_default(), SHOWN);
            let options = shown
                .into_iter()
                .map(|branch| Choice { value: branch.name.clone(), label: branch.name.clone(), detail: branch.remote_only.then(|| "remote".to_owned()) })
                .collect();
            let picker = combobox(
                Combobox { label: "Branch", placeholder: "Find a branch", value: String::new(), query: Some(query), options, empty: "No branch matches", auto_focus: true },
                searched,
                pick,
            );
            stack().prop("gap", "s").child(picker).maybe((total > SHOWN).then(|| muted(format!("{} more: type to narrow the list", total - SHOWN)).prop("size", "small")))
        }
    };
    let head = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .child(text("Switch branch").prop("weight", "strong"))
        .child(stack().prop("grow", true))
        .child(
            button("", "ghost", false, close)
                .prop("size", "icon-sm")
                .prop("label", "Close")
                .prop("title", "Close")
                .child(icon(14, 2.0, &[CLOSE]).prop("size", 12)),
        );
    Some(card().prop("variant", "raised").key("branches").child(stack().prop("gap", "s").child(head).child(body)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    const REFS: &str = "refs/heads/feature/pay\nrefs/heads/main\nrefs/heads/Release-2\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/remotes/origin/feature/search\nrefs/remotes/upstream/feature/search\n";

    fn names(branches: &[Branch]) -> Vec<&str> {
        branches.iter().map(|branch| branch.name.as_str()).collect()
    }

    #[test]
    fn local_branches_come_first_then_the_ones_only_on_a_remote_each_once_without_the_current_one() {
        let all = parse_branches(REFS, "main");
        assert_eq!(names(&all), ["feature/pay", "Release-2", "feature/search"]);
        assert!(!all[0].remote_only && !all[1].remote_only && all[2].remote_only);
        // `origin/HEAD` is no branch, and a branch on a remote and here is the local one.
        assert!(!names(&parse_branches(REFS, "")).contains(&"HEAD"));
        assert!(parse_branches(REFS, "").iter().any(|branch| branch.name == "main" && !branch.remote_only));
    }

    #[test]
    fn typing_narrows_the_list_whatever_the_case_and_caps_what_shows() {
        let all = parse_branches(REFS, "main");
        assert_eq!(names(&matching(&all, "", 12).0.into_iter().cloned().collect::<Vec<_>>()), ["feature/pay", "Release-2", "feature/search"]);
        let (found, total) = matching(&all, " FEATURE", 12);
        assert_eq!((found.iter().map(|branch| branch.name.as_str()).collect::<Vec<_>>(), total), (vec!["feature/pay", "feature/search"], 2));
        assert_eq!(matching(&all, "release", 12).1, 1);
        assert_eq!(matching(&all, "nothing like it", 12), (vec![], 0));
        let (shown, total) = matching(&all, "", 2);
        assert_eq!((shown.len(), total), (2, 3));
    }

    #[test]
    fn git_switch_is_done_on_code_0_and_otherwise_says_what_git_said() {
        assert_eq!(outcome(Ok(json!({ "code": 0, "stdout": "", "stderr": "Switched to branch 'x'\n" }))), Ok(()));
        let blocked = "error: Your local changes to the following files would be overwritten by checkout:\n\tsrc/a.rs\nPlease commit your changes or stash them before you switch branches.\nAborting\n";
        assert_eq!(outcome(Ok(json!({ "code": 1, "stdout": "", "stderr": blocked }))), Err(blocked.trim().to_owned()));
        assert_eq!(outcome(Ok(json!({ "code": 128, "stdout": "fatal on stdout\n", "stderr": "  " }))), Err("fatal on stdout".to_owned()));
        assert_eq!(outcome(Ok(json!({ "code": 1 }))), Err("git switch failed".to_owned()));
        assert_eq!(outcome(Err("git is not installed or not on PATH".into())), Err("git is not installed or not on PATH".to_owned()));
    }

    /// Wings, as the switcher asks it: every `exec`'s arguments kept, git's answers by subcommand.
    fn git(switched: Value) -> Rc<RefCell<Vec<Vec<String>>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            if method != "exec" {
                return Ok(Value::Null);
            }
            let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|arg| arg.as_str().unwrap().to_owned()).collect();
            kept.borrow_mut().push(args.clone());
            Ok(match args[0].as_str() {
                "for-each-ref" => json!({ "code": 0, "stdout": REFS, "stderr": "" }),
                "switch" => switched.clone(),
                "rev-parse" => json!({ "code": 0, "stdout": "feature/pay\n", "stderr": "" }),
                _ => json!({ "code": 1, "stdout": "", "stderr": "" }),
            })
        });
        calls
    }

    fn ready(working: bool) {
        close();
        chat::with(|c| {
            c.cwd = "/work/shop".into();
            c.branch = Some("main".into());
            c.session.working = working;
        });
    }

    fn page(c: &chat::Chat) -> String {
        wings_plugin::ui::tree_json(panel(c).unwrap_or_else(|| stack())).to_string()
    }

    #[test]
    fn the_list_reads_the_branches_when_it_opens_and_a_pick_switches_and_refreshes_the_header() {
        let calls = git(json!({ "code": 0, "stdout": "", "stderr": "Switched to branch 'feature/pay'\n" }));
        ready(false);
        toggle();
        task::block_on(async {});
        let shown = chat::with(|c| page(c));
        assert!(shown.contains("feature/pay") && shown.contains("feature/search") && shown.contains("remote") && !shown.contains("\"main\""), "{shown}");
        assert_eq!(calls.borrow()[0], ["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]);
        // A pick closes the list, runs `git switch` and reads the branch again: the header's.
        pick("feature/pay");
        task::block_on(async {});
        assert!(calls.borrow().iter().any(|args| args == &["switch", "feature/pay"]));
        assert!(calls.borrow().iter().any(|args| args[0] == "rev-parse"), "the header's branch is read again");
        assert_eq!(chat::with(|c| c.branch.clone()), Some("feature/pay".to_owned()));
        assert!(chat::with(|c| panel(c)).is_none());
        assert!(!chat::with(|c| c.session.items.iter().any(|item| matches!(item, Item::Error(_)))));
    }

    #[test]
    fn a_switch_git_refuses_shows_git_s_error_in_the_conversation_and_the_branch_stays() {
        let blocked = "error: Your local changes to the following files would be overwritten by checkout:\n\tsrc/a.rs\nAborting";
        let calls = git(json!({ "code": 1, "stdout": "", "stderr": blocked }));
        ready(false);
        chat::with(|c| c.session.items.clear());
        pick("feature/pay");
        task::block_on(async {});
        let said = chat::with(|c| c.session.items.iter().filter_map(|item| if let Item::Error(said) = item { Some(said.clone()) } else { None }).collect::<Vec<_>>());
        assert_eq!(said.len(), 1);
        assert!(said[0].starts_with("Couldn't switch to feature/pay: error: Your local changes") && said[0].contains("src/a.rs"), "{said:?}");
        // No force, no stash, and no re-read of the branch: nothing changed.
        assert_eq!(calls.borrow().iter().filter(|args| args[0] == "switch").count(), 1);
        assert!(calls.borrow().iter().all(|args| args[0] != "rev-parse" && args[0] != "stash" && !args.iter().any(|arg| arg == "--force" || arg == "-f")));
        assert_eq!(chat::with(|c| c.branch.clone()), Some("main".to_owned()));
    }

    #[test]
    fn while_claude_works_the_button_is_off_with_a_title_that_says_why_and_a_pick_does_nothing() {
        let calls = git(json!({ "code": 0, "stdout": "", "stderr": "" }));
        ready(true);
        let node = chat::with(|c| button_node(c)).unwrap();
        let drawn = wings_plugin::ui::tree_json(node).to_string();
        assert!(drawn.contains("\"disabled\":true") && drawn.contains("Claude is working on a turn"), "{drawn}");
        toggle();
        pick("feature/pay");
        task::block_on(async {});
        assert!(calls.borrow().is_empty());
        assert!(chat::with(|c| panel(c)).is_none());
        // Idle, it is on, and names the branch; narrow, only the icon is left.
        ready(false);
        let wide = wings_plugin::ui::tree_json(chat::with(|c| button_node(c)).unwrap()).to_string();
        assert!(!wide.contains("\"disabled\":true") && wide.contains("\"main\"") && wide.contains("Branch main. Switch branch"), "{wide}");
        chat::with(|c| c.narrow = Some(true));
        let narrow = wings_plugin::ui::tree_json(chat::with(|c| button_node(c)).unwrap()).to_string();
        assert!(!narrow.contains("\"main\"") && narrow.contains("Branch main. Switch branch"), "{narrow}");
        chat::with(|c| c.narrow = None);
        // Not on a branch (no repo, or a detached head): no button.
        chat::with(|c| c.branch = None);
        assert!(chat::with(|c| button_node(c)).is_none());
    }
}
