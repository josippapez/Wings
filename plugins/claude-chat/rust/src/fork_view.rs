//! The buttons and the dialog of `fork.rs`, and what they do: Fork in the header, Fork from here beside each of your
//! messages with its confirmation, and the folders you added in the side panel, each with a Remove. Each restart answers one
//! click or key.

use wings_plugin::{
    format::truncate,
    host, task,
    ui::{self, Node, card, dialog as panel, icon, muted, row, stack, text},
};

use crate::{
    chat::{self, Chat, State},
    fork::{Launch, add, resolve},
    rewind_view,
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
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";
const WIDTH: u32 = 520;

/// The header's Fork button, once there is a session to copy. It waits while Claude works: the copy starts Claude Code
/// again, which would end the turn.
pub fn header_button(c: &Chat) -> Option<Node> {
    (c.session.id.is_some() && c.state == State::Running).then(|| {
        ui::button("", "ghost", false, ask_fork)
            .prop("size", "icon-sm")
            .prop("label", "Fork")
            .prop("title", "Fork: go on in a copy of this session, and leave this one as it is")
            .prop("disabled", c.session.working)
            .child(icon(24, 2.0, &FORK).prop("size", 16))
    })
}

/// The buttons beside your message at `at`: Rewind and Fork from here, each once Claude Code knows the message by a uuid.
pub fn message_actions(at: usize, c: &Chat) -> Option<Node> {
    let rewind = rewind_view::button(at, c);
    let forking = c.session.prompts.iter().find(|prompt| prompt.at == at).filter(|_| c.session.id.is_some() && c.state == State::Running).map(|prompt| {
        let uuid = prompt.uuid.clone();
        ui::button("", "ghost", false, move || ask_fork_from(&uuid))
            .prop("size", "icon-sm")
            .prop("label", "Fork from here")
            .prop("title", "Fork from here: a copy of this session as it was before this message")
            .prop("disabled", c.session.working)
            .child(icon(24, 2.0, &FORK).prop("size", 14))
    });
    match (rewind, forking) {
        (None, None) => None,
        (rewind, forking) => Some(row().prop("gap", "xs").prop("align", "center").maybe(rewind).maybe(forking)),
    }
}

/// Cancel and Fork, under what a dialog says.
fn buttons(c: &Chat, cancel: fn(), fork: fn()) -> Node {
    row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("justify", "end")
        .prop("align", "center")
        .child(ui::button("Cancel", "ghost", false, cancel))
        .child(ui::button("Fork", "default", false, fork).prop("disabled", c.session.working))
}

/// The dialog that asks before the header's Fork copies the whole session, while it is open.
pub fn header_dialog(c: &Chat) -> Option<Node> {
    c.fork_ask.then(|| {
        panel("Fork this session", WIDTH, close_ask)
            .child(muted("A fork is a new session that starts with this whole conversation, so you can take it another way.").prop("size", "small"))
            .child(muted("This one stays as it is, in Past sessions. Your files stay as they are.").prop("size", "small"))
            .child(buttons(c, close_ask, fork))
    })
}

/// The dialog that asks before forking before a message, while it is open.
pub fn dialog(c: &Chat) -> Option<Node> {
    let uuid = c.fork_from.as_deref()?;
    let said = c.session.prompts.iter().find(|prompt| prompt.uuid == uuid).and_then(|prompt| match c.session.items.get(prompt.at) {
        Some(Item::User(said) | Item::UserImages(said, _)) => Some(said.trim()),
        _ => None,
    });
    let said = match said {
        Some(said) if !said.is_empty() => text(truncate(said, 600)),
        _ => muted("A message of pictures only"),
    };
    let actions = buttons(c, close_fork_from, fork_from);
    Some(
        panel("Fork from here", WIDTH, close_fork_from)
            .child(muted("Start a new chat from this session as it was before this message of yours:").prop("size", "small"))
            .child(card().child(said))
            .child(muted("The new chat has the conversation up to there, and this message goes back in the box to edit. This session stays as it is, in Past sessions. Your files stay as they are.").prop("size", "small"))
            .child(actions),
    )
}

/// The folders you added with /add-dir, each with a Remove, none when there are none.
pub fn folders(c: &Chat) -> Option<Node> {
    if c.dirs.is_empty() {
        return None;
    }
    let rows = c.dirs.iter().map(|dir| {
        let remove = dir.clone();
        row()
            .prop("gap", "xs")
            .prop("align", "center")
            .key(dir.as_str())
            .child(stack().prop("grow", true).child(text(dir.as_str()).prop("size", "small").prop("truncate", true).prop("title", dir.as_str())))
            .child(
                ui::button("", "ghost", false, move || remove_dir(&remove))
                    .prop("size", "icon-sm")
                    .prop("label", "Remove folder")
                    .prop("title", "Remove this folder")
                    .prop("disabled", c.session.working)
                    .child(icon(14, 2.0, &[CLOSE])),
            )
    });
    Some(stack().prop("gap", "xs").child(Node::new("Heading").text("Folders")).child(stack().prop("gap", "none").children(rows)))
}

/// Why the chat can't start Claude Code again now, or None. A turn that runs would be lost, so it waits.
pub fn busy(c: &Chat) -> Option<&'static str> {
    if c.session.working {
        Some("Claude is working. Do that when its turn has ended.")
    } else if c.state != State::Running {
        Some("Claude Code isn't running in this chat.")
    } else {
        None
    }
}

/// Tells you in the conversation why something didn't happen.
fn say(why: impl Into<String>) {
    let why = why.into();
    chat::with(|c| c.session.items.push(Item::Error(why)));
}

/// Asks before the header's Fork copies the session.
pub fn ask_fork() {
    chat::with(|c| c.fork_ask = true);
}

pub fn close_ask() {
    chat::with(|c| c.fork_ask = false);
}

/// Starts the session again as a copy of itself, under a new id, from the dialog's Fork button.
pub fn fork() {
    let started = chat::with(|c| {
        c.fork_ask = false;
        if chat::in_use(c) {
            if let Some(id) = c.session.id.clone() {
                return Some(Err(chat::fork_beside(c, &id, None)));
            }
        }
        if let Some(why) = busy(c) {
            c.session.items.push(Item::Error(why.into()));
            return None;
        }
        c.session.id.clone().map(Ok)
    });
    match started {
        Some(Ok(id)) => chat::launch_with(Some(id), Launch { fork: true, before: None }),
        Some(Err(args)) => chat::open_beside(&args),
        None => {}
    }
}

/// Asks before forking before the message `uuid`, from the button beside it.
pub fn ask_fork_from(uuid: &str) {
    chat::with(|c| c.fork_from = Some(uuid.to_owned()));
}

pub fn close_fork_from() {
    chat::with(|c| c.fork_from = None);
}

/// Forks before the message the dialog is about: a copy of the session without it and what followed, which the box gets
/// back to edit. The original stays as it was.
pub fn fork_from() {
    let started = chat::with(|c| {
        let uuid = c.fork_from.take()?;
        if chat::in_use(c) {
            if let Some(id) = c.session.id.clone() {
                return Some(Err(chat::fork_beside(c, &id, Some(&uuid))));
            }
        }
        if let Some(why) = busy(c) {
            c.session.items.push(Item::Error(why.into()));
            return None;
        }
        Some(Ok((c.session.id.clone()?, uuid)))
    });
    match started {
        Some(Ok((id, uuid))) => chat::launch_with(Some(id), Launch { fork: true, before: Some(uuid) }),
        Some(Err(args)) => chat::open_beside(&args),
        None => {}
    }
}

/// `/add-dir <path>`, from your Enter: checks the folder is there, then goes on with the same session with Claude Code
/// let into it. Claude Code takes any path for `--add-dir` and says nothing of one that isn't there.
pub fn add_dir(path: String) {
    let (cwd, blocked) = chat::with(|c| (c.cwd.clone(), busy(c)));
    if let Some(why) = blocked {
        return say(why);
    }
    if path.is_empty() {
        return say("Say which folder: /add-dir <path>");
    }
    let dir = resolve(&cwd, &path);
    task::spawn(async move {
        match host::exec_in(&cwd, "test", &["-d", dir.as_str()]).await {
            Ok(out) if out["code"] == 0 => restart_with(|c| {
                add(&mut c.dirs, dir.clone());
                c.draft.clear();
            }),
            Ok(_) => say(format!("{dir} isn't a folder.")),
            Err(error) => say(format!("Couldn't check {dir}: {error}")),
        }
    });
}

/// Takes a folder back out, from its Remove button, and goes on with the same session.
pub fn remove_dir(dir: &str) {
    if let Some(why) = chat::with(|c| busy(c)) {
        return say(why);
    }
    restart_with(|c| c.dirs.retain(|kept| kept != dir));
}

/// Changes the folders and starts Claude Code again on the same session, unless a turn began since the check.
fn restart_with(change: impl FnOnce(&mut Chat)) {
    let resume = chat::with(|c| {
        if let Some(why) = busy(c) {
            c.session.items.push(Item::Error(why.into()));
            return None;
        }
        change(c);
        Some(c.session.id.clone())
    });
    if let Some(resume) = resume {
        chat::launch_with(resume, Launch::default());
    }
}
