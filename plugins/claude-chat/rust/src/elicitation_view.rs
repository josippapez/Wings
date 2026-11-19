//! The card for what an MCP server asks you (`elicitation.rs`): its message, then the form to fill or the link to open,
//! with Submit (or Open and continue), Decline and Cancel. It sits at the foot of the conversation while Claude waits.

use wings_plugin::{
    host,
    ui::{Node, button, checkbox, confirm, muted, notice, row, select, stack, text, text_input},
};

use crate::{
    chat::{self, Chat},
    elicitation::{Action, Elicitation, Field, Input, Kind},
};

/// A card for each question waiting, oldest first.
pub fn cards(c: &Chat) -> Vec<Node> {
    c.session.ops.elicitations.iter().map(card).collect()
}

fn card(asked: &Elicitation) -> Node {
    let mut body = vec![text(asked.message.as_str())];
    match &asked.kind {
        Kind::Form(fields) => {
            body.extend(fields.iter().map(|field| field_node(asked, field)));
            // What stops Submit, so a disabled button isn't a mystery. A missing required field is not said before you have
            // done anything: its label already says it is required, and the button's tooltip says which.
            if let Err(why) = asked.content() {
                if !why.ends_with(" is required.") {
                    body.push(muted(why).prop("size", "small"));
                }
            }
        }
        Kind::Url(url) => {
            body.push(text(url.as_str()).prop("mono", true).prop("size", "small").prop("truncate", true).prop("title", url.as_str()));
            body.push(muted("It opens in your browser. Check the address first.").prop("size", "small"));
        }
        Kind::Other(why) => body.push(notice("warn", why.as_str())),
    }
    let (decline, cancel) = (asked.request.clone(), asked.request.clone());
    let accept = asked.request.clone();
    let go = match &asked.kind {
        Kind::Form(_) => {
            let ready = asked.content();
            let held = ready.as_ref().err().cloned().unwrap_or_default();
            Some(button("Submit", "default", false, move || answer(&accept, Action::Accept)).prop("disabled", ready.is_err()).prop("title", held))
        }
        Kind::Url(_) => Some(button("Open and continue", "default", false, move || open_and_continue(&accept))),
        Kind::Other(_) => None,
    };
    let title = format!("Request from {}", asked.server);
    // Cancel, the quieter Decline, then the one that goes ahead, at the right as a dialog's are.
    let actions = row()
        .prop("grow", true)
        .prop("gap", "s")
        .prop("justify", "end")
        .child(button("Cancel", "ghost", false, move || answer(&cancel, Action::Cancel)))
        .child(button("Decline", "outline", false, move || answer(&decline, Action::Decline)))
        .maybe(go);
    confirm(title, body, [actions]).key(format!("elicitation-{}", asked.request))
}

/// One field of the form: its control, and what the server says about it.
fn field_node(asked: &Elicitation, field: &Field) -> Node {
    let (request, key) = (asked.request.clone(), field.key.clone());
    let label = if field.required { format!("{} (required)", field.title) } else { field.title.clone() };
    let value = asked.value(&field.key);
    let control = match &field.input {
        Input::Text => text_input(&label, value, move |said| edit(&request, &key, said)),
        Input::Number { whole } => text_input(&label, value, move |said| edit(&request, &key, said)).prop("placeholder", if *whole { "A whole number" } else { "A number" }),
        Input::Bool => checkbox(label.as_str(), value == "true", move |on| edit(&request, &key, on.to_string())),
        Input::Choice(options) => {
            let first = if field.required { "Choose" } else { "None" };
            let options: Vec<(String, String)> = std::iter::once((String::new(), first.to_owned())).chain(options.iter().cloned()).collect();
            select(&label, &options, value, move |picked| edit(&request, &key, picked.to_owned()))
        }
    };
    // A checkbox names itself; a box or a menu gets its name over it.
    let name = (!matches!(field.input, Input::Bool)).then(|| text(label.as_str()).prop("size", "small").prop("weight", "medium"));
    let said = Some(field.description.as_str()).filter(|said| !said.is_empty()).map(|said| muted(said).prop("size", "small"));
    stack().prop("gap", "xs").key(field.key.as_str()).maybe(name).child(control).maybe(said)
}

fn edit(request: &str, key: &str, value: String) {
    chat::with(|c| c.session.edit_elicitation(request, key, value));
}

/// Answers with `action`: one write, from the click.
fn answer(request: &str, action: Action) {
    let request = request.to_owned();
    chat::write(move |c, _| c.session.answer_elicitation(&request, action));
}

/// Opens the server's link and tells it you did, from the one click.
fn open_and_continue(request: &str) {
    let url = chat::with(|c| c.session.ops.elicitations.iter().find(|asked| asked.request == request).and_then(|asked| asked.url().map(str::to_owned)));
    if let Some(url) = url {
        host::open_url(&url);
        answer(request, Action::Accept);
    }
}
