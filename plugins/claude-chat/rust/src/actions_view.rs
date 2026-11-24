//! The buttons and the dialog of `actions.rs`: Run in background on a command that runs, and Message on a background
//! agent in the side panel.

use wings_plugin::{
    format::truncate,
    host,
    ui::{self, Node, dialog as panel, muted, row, text_input},
};

use crate::{
    actions::{self, agent_message},
    chat::{self, Chat, State},
    session::{Item, Task, Tool},
};

const WIDTH: u32 = 520;

/// Run in background on a command Claude is waiting for. Not before it is allowed, when nothing runs yet.
pub fn background_button(tool: &Tool, c: &Chat) -> Option<Node> {
    let waiting = c.session.items.iter().any(|item| matches!(item, Item::Ask(ask) if ask.answer.is_none()));
    (tool.name == "Bash" && c.session.runs(tool) && c.session.working && c.state == State::Running && !waiting).then(|| {
        let id = tool.id.clone();
        let moving = c.session.ops.moving.contains(&tool.id);
        ui::button("Run in background", "outline", moving, move || move_to_background(&id))
            .prop("size", "sm")
            .prop("title", "Claude goes on without waiting for it, and it keeps running as a background task")
            .key(format!("background-{}", tool.id))
    })
}

/// The same button beside the status line, for the command Claude waits on now: its step is folded away until you open it.
pub fn status_button(c: &Chat) -> Option<Node> {
    let tool = c.session.items.iter().rev().find_map(|item| match item {
        Item::Tool(tool) if c.session.runs(tool) => Some(tool),
        _ => None,
    })?;
    background_button(tool, c)
}

/// Lucide's `message-square`, in a 24-point box.
const MESSAGE: &str = "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z";

/// A background agent's Message button, for one that still runs.
pub fn message_button(task: &Task) -> Option<Node> {
    actions::is_running_agent(task).then(|| {
        let id = task.id.clone();
        ui::button("", "ghost", false, move || open_message(&id))
            .prop("size", "icon-sm")
            .prop("label", "Message")
            .prop("title", "Have Claude pass a message on to this agent")
            .child(ui::icon(24, 2.0, &[MESSAGE]).prop("size", 14))
    })
}

/// The dialog for a message to a background agent, while it is open and the agent still runs.
pub fn message_dialog(c: &Chat) -> Option<Node> {
    let (task, said) = c.message_to.as_ref()?;
    let found = c.session.tasks.iter().find(|known| known.id == *task && actions::is_running_agent(known))?;
    let name = if found.description.is_empty() { "Background agent" } else { found.description.as_str() };
    let buttons = row()
        .prop("gap", "s")
        .prop("wrap", true)
        .prop("justify", "end")
        .prop("align", "center")
        .child(ui::button("Cancel", "ghost", false, close_message))
        .child(ui::button("Send", "default", false, send_message).prop("disabled", said.trim().is_empty()));
    Some(
        panel(&truncate(name, 60), WIDTH, close_message)
            .child(muted("Claude passes your message on to this agent. It shows in the chat as a message of yours.").prop("size", "small"))
            .child(text_input("Message", said, edit_message).prop("placeholder", "What should the agent know?").prop("autoFocus", true))
            .child(buttons),
    )
}

/// Moves the command of the tool call `id` to the background, from the button on it.
pub fn move_to_background(id: &str) {
    let id = id.to_owned();
    chat::write(move |c, _| c.session.background(&id));
}

/// Opens the message box for the background agent `task`, from its Message button.
pub fn open_message(task: &str) {
    chat::with(|c| c.message_to = Some((task.to_owned(), String::new())));
}

pub fn edit_message(said: String) {
    chat::with(|c| {
        if let Some((_, text)) = &mut c.message_to {
            *text = said;
        }
    });
}

pub fn close_message() {
    chat::with(|c| c.message_to = None);
}

/// Sends the message to Claude, who passes it on to the agent: at once when Claude is idle, else queued as any message.
pub fn send_message() {
    chat::write(|c, n| {
        let (task, said) = c.message_to.clone()?;
        let text = agent_message(&c.session, &task, &said)?;
        c.message_to = None;
        let stamp = host::now() as u64;
        if c.session.working {
            return Some(c.session.queue(&text, Vec::new(), stamp, n));
        }
        let line = c.session.send(&text, &[], stamp, n);
        c.doing = (c.session.activity(), stamp);
        Some(line)
    });
}
