//! The cards the sidebar and the popover draw from `State`.

use crate::{
    accounts::{Account, Loaded, Project, account_label, name_problem, sign_in_label, start_label},
    actions::{self, CHANGED, DEFAULT},
    state::{State, set_project, with},
};
use serde_json::json;
use wings_plugin::{
    host, messages,
    task::spawn,
    ui::{Node, badge, button, card, checkbox, loading, muted, notice, row, select, slot, stack, text, text_input, title},
};

const OPEN_A_PROJECT: &str = "Open a project to choose its Claude account.";

/// Follows the project on screen and keeps the accounts loaded: when Wings is in the background as the page starts, and
/// whenever it comes to the front (which is also when a popover is shown again) or the other page changed something.
pub fn start(view: fn() -> Vec<Node>) {
    host::on("project", |_, project| set_project(project));
    // `on_focus` says the current state once, so a page that starts in front loads from there.
    if !host::focused() {
        spawn(actions::reload());
    }
    host::on_focus(|focused| {
        if focused {
            spawn(actions::reload());
        }
    });
    messages::on(CHANGED, |_| spawn(actions::reload()));
    wings_plugin::run(view);
}

/// What shows until there is a project and a list to draw, or why there is no list.
pub fn gate(s: &State) -> Option<Vec<Node>> {
    if s.project.is_none() || (s.data.is_none() && s.error.is_none()) {
        return Some(vec![loading()]);
    }
    if s.data.is_some() {
        return None;
    }
    let again = button("Try again", "secondary", false, || spawn(actions::reload()));
    Some(vec![
        Node::new("Empty")
            .child(slot("title", text("Couldn't read the Claude accounts")))
            .child(slot("description", text(s.error.clone().unwrap_or_default())))
            .child(slot("children", again)),
    ])
}

fn small(node: Node) -> Node {
    node.prop("size", "small")
}

fn heading(title: &str) -> Node {
    Node::new("Heading").text(title)
}

/// The sidebar: the project on screen and its account, then every account and who uses it, and a form to add one.
pub fn sidebar() -> Vec<Node> {
    let s = with(|s| s.clone());
    if let Some(shown) = gate(&s) {
        return shown;
    }
    let Some(data) = &s.data else { return vec![] };
    let project = s.project.as_ref().and_then(|p| p.as_ref());
    let mut out = vec![];
    if let Some(error) = &s.error {
        out.push(notice("danger", error.as_str()).key("error"));
    }
    out.push(heading("Project").key("project-title"));
    out.push(match project {
        Some(project) => project_account(project, data, &s, false).key("project"),
        None => muted(OPEN_A_PROJECT).key("none"),
    });
    let was_adding = s.adding;
    let adding = button("Add account", "ghost", false, move || {
        if was_adding {
            actions::close_form();
        } else {
            with(|s| s.adding = true);
        }
    })
    .prop("size", "sm")
    .prop("disabled", project.is_none())
    .prop("title", if project.is_none() { "Open a project first" } else { "" })
    .prop("aria-expanded", was_adding);
    out.push(row().child(heading("Accounts")).child(Node::new("Text").prop("grow", true)).child(adding).key("accounts-title"));
    if let (true, Some(project)) = (s.adding, project) {
        out.push(add_account(project, data, &s).key("add"));
    }
    out.push(account_list(data, project, &s).key("accounts"));
    out
}

/// The popover under the title bar button: the project on screen and its account, to switch at a glance.
pub fn popover() -> Vec<Node> {
    let s = with(|s| s.clone());
    if let Some(shown) = gate(&s) {
        return shown;
    }
    let Some(data) = &s.data else { return vec![] };
    let all = button("All accounts", "ghost", false, || drop(host::call("openView", json!({ "id": "accounts", "newTab": false })))).prop("size", "sm");
    let mut out = vec![row().child(title("Claude accounts").prop("truncate", true).prop("grow", true)).child(all).key("title")];
    if let Some(error) = &s.error {
        out.push(notice("danger", error.as_str()).key("error"));
    }
    match s.project.as_ref().and_then(|p| p.as_ref()) {
        Some(project) => out.push(project_account(project, data, &s, true).key("project")),
        None => out.push(muted(OPEN_A_PROJECT).key("none")),
    }
    out
}

/// The project on screen and the account its new terminals use, with a picker, and a way into Claude once it changed.
/// `compact`, for the popover, has no path under the project's name.
fn project_account(project: &Project, data: &Loaded, s: &State, compact: bool) -> Node {
    let shown: Option<&str> = match &s.picked {
        Some(picked) => picked.as_deref(),
        None => data.account_of(&project.id),
    };
    let current = data.accounts.iter().find(|a| a.name.as_deref() == shown);
    let (swap, open) = (s.action("swap"), s.action("open"));
    let open_busy = open.busy;
    let error = swap.error.or(open.error);

    let options: Vec<(String, String)> = data.accounts.iter().map(|a| (a.name.clone().unwrap_or_else(|| DEFAULT.into()), account_label(a))).collect();
    let picked_from = project.clone();
    let picker = select("Claude account", &options, shown.unwrap_or(DEFAULT), move |value| spawn(actions::pick(picked_from.clone(), value.to_owned())));

    let heading = stack().prop("gap", "xs").child(title(project.name.as_str()).prop("size", "large").prop("truncate", true)).maybe(
        (!compact).then(|| small(muted(project.path.as_str())).prop("mono", true).prop("truncate", true).prop("title", project.path.as_str())),
    );
    let mut card = card()
        .child(heading)
        .child(stack().prop("gap", "xs").child(picker).maybe(s.note.is_none().then(|| small(muted("New terminals in this project use it.")))))
        .maybe(current.filter(|a| !a.signed_in).map(|account| {
            not_signed_in(account, Some(&project.path), "card", "Not signed in. New Claude sessions here won't use your plan until you sign in.".into(), s)
        }))
        .maybe(start_claude(project, data, s))
        .maybe(error.map(|error| notice("danger", error)));
    if let Some(note) = &s.note {
        let message = if note.added {
            format!("Added {} for {}. Claude asks you to sign in the first time it runs with it.", note.label, project.name)
        } else {
            format!("{} uses {} now. New terminals start with it. Claude sessions that are open keep their account.", project.name, note.label)
        };
        let again = project.clone();
        card = card.child(notice("success", message)).child(
            row()
                .child(button("Open Claude", "secondary", open_busy, move || spawn(actions::open_claude(again.clone()))).prop("size", "sm"))
                .child(small(muted("in a new terminal"))),
        );
    }
    card
}

/// What a screen reader calls a "Start Claude as" button.
pub fn start_aria_label(name: &str, signed_out: bool) -> String {
    format!("Start Claude as {name}{}", if signed_out { ", not signed in" } else { "" })
}

/// What a "Start Claude as" button says when pointed at. `default` is whether the project uses the default account.
pub fn start_title(name: &str, signed_out: bool, default: bool) -> String {
    format!(
        "Start Claude as {name} in a new terminal. {}{}",
        if signed_out { "It is not signed in, so Claude won't use its plan. " } else { "" },
        if default { "The project keeps the default account." } else { "The project keeps its own account." }
    )
}

/// One compact button for each account the project doesn't use, starting Claude in a new terminal there as that account.
/// The project keeps its own account; its terminals and "Open Claude" are for that one.
fn start_claude(project: &Project, data: &Loaded, s: &State) -> Option<Node> {
    let now = data.account_of(&project.id);
    let others: Vec<&Account> = data.accounts.iter().filter(|a| a.name.as_deref() != now).collect();
    if others.is_empty() {
        return None;
    }
    let start = s.action("start");
    let buttons = others.into_iter().map(|account| {
        let key = account.name.clone().unwrap_or_default();
        let label = start_label(account);
        let (project, name) = (project.clone(), account.name.clone());
        let busy = start.busy && s.starting.as_deref() == Some(key.as_str());
        let shown = if account.signed_in { label } else { format!("{label} · not signed in") };
        button(shown, "outline", busy, move || spawn(actions::start_claude(project.clone(), name.clone()))).prop("size", "xs").prop("label", start_aria_label(&start_label(account), !account.signed_in)).prop("title", start_title(&start_label(account), !account.signed_in, now.is_none())).key(key)
    });
    Some(
        stack()
            .prop("gap", "s")
            .child(small(muted("Start Claude as")))
            .child(row().prop("wrap", true).prop("gap", "s").children(buttons))
            .maybe(start.error.map(|error| notice("danger", error))),
    )
}

/// A warning that `account` has no sign-in, with a button that opens a terminal as the account to sign in. `cwd` is the
/// project to open it in, if there is one. `place` tells this one from the same account's elsewhere on the page.
fn not_signed_in(account: &Account, cwd: Option<&str>, place: &str, message: String, s: &State) -> Node {
    let key = format!("signin:{place}:{}", account.name.as_deref().unwrap_or(DEFAULT));
    let action = s.action(&key);
    let (name, folder) = (account.name.clone(), cwd.map(str::to_owned));
    let mut sign_in = button("Sign in", "outline", action.busy, move || {
        if let Some(folder) = &folder {
            spawn(actions::sign_in(key.clone(), name.clone(), folder.clone()));
        }
    })
    .prop("size", "xs")
    .prop("title", if cwd.is_none() { "Open a project first" } else { "Opens a terminal as this account running claude auth login" });
    if cwd.is_none() {
        sign_in = sign_in.prop("disabled", true);
    }
    stack()
        .prop("gap", "xs")
        .child(row().child(small(text(message)).prop("tone", "warn").prop("grow", true).prop("title", account.dir.as_str())).child(sign_in))
        .maybe(action.error.map(|error| notice("danger", error)))
}

/// Each account with who it is, or that it needs signing in, and the projects that use it. `project` is the one on screen, if any.
fn account_list(data: &Loaded, project: Option<&Project>, s: &State) -> Node {
    let now = project.map(|p| data.account_of(&p.id));
    Node::new("List").children(data.accounts.iter().map(|account| {
        let users = data.projects_using(account);
        let name = account.name.as_deref();
        let here = now.is_some() && now == Some(name);
        let who = if account.signed_in {
            small(muted(sign_in_label(account))).prop("truncate", true).prop("title", account.dir.as_str())
        } else {
            not_signed_in(account, project.map(|p| p.path.as_str()), "list", sign_in_label(account), s)
        };
        let used = if users.is_empty() { "No projects use it".to_owned() } else { format!("Used by {}", users.join(", ")) };
        Node::new("ListItem").key(name.unwrap_or(DEFAULT)).child(
            stack()
                .prop("gap", "xs")
                .prop("grow", true)
                .child(row().child(text(name.unwrap_or("Default")).prop("weight", "medium").prop("truncate", true)).maybe(here.then(|| badge("This project", Some("info")).prop("size", "sm"))))
                .child(who)
                .child(small(muted(used)).prop("clamp", true).prop("title", users.join(", ")))
                .maybe(name.map(|name| share_setup(name, account.shares_setup, s))),
        )
    }))
}

/// Whether a named account uses the default account's settings, plugins, skills and rules.
fn share_setup(name: &str, on: bool, s: &State) -> Node {
    let action = s.action(&format!("share:{name}"));
    let owned = name.to_owned();
    // The box shows the choice just made while Wings links the folders.
    let checked = s.shares.get(name).copied().unwrap_or(on);
    stack()
        .prop("gap", "xs")
        .child(checkbox("Use my default setup", checked, move |next| spawn(actions::share_setup(owned.clone(), next))).prop("disabled", action.busy))
        .child(small(muted("Uses the default account's plugins, skills and rules, and a copy of its settings that keeps this account's own model. This account's own files are kept aside and come back when you turn it off.")))
        .maybe(action.error.map(|error| notice("danger", error)))
}

/// Why the name typed can't be used, as the form says it: wrong characters show as they are typed; an empty name waits
/// until Add is pressed.
pub fn form_problem(name: &str, submitted: bool, existing: &[Account]) -> Option<String> {
    if !name.is_empty() {
        name_problem(name, existing)
    } else if submitted {
        Some("Enter a name for the account.".into())
    } else {
        None
    }
}

/// Asks for a name, gives the project that account and opens Claude in a new terminal there to sign in.
fn add_account(project: &Project, data: &Loaded, s: &State) -> Node {
    let action = s.action("add");
    let problem = form_problem(&s.name, s.submitted, &data.accounts).or(action.error);
    let (for_enter, for_button) = (project.clone(), project.clone());
    let field = text_input("Account name", &s.name, |name| with(|s| s.name = name))
        .prop("placeholder", "work")
        .prop("autoFocus", true)
        .on("onKeyDown", move |event| {
            if event["key"] == "Enter" {
                spawn(actions::add_account(for_enter.clone()));
            }
        });
    card()
        .child(
            stack()
                .prop("gap", "xs")
                .child(field)
                .child(small(muted("Lowercase letters, digits and dashes, like work or client-a.")))
                .maybe(problem.map(|problem| small(text(problem)).prop("tone", "danger"))),
        )
        .child(small(muted(format!(
            "{} switches to this account, and Claude opens in a new terminal there so you can sign in. It starts out with your default account's settings, plugins, skills and rules.",
            project.name
        ))))
        .child(
            row()
                .prop("justify", "end")
                .child(button("Cancel", "ghost", false, actions::close_form))
                .child(button("Add and sign in", "default", action.busy, move || spawn(actions::add_account(for_button.clone())))),
        )
}
