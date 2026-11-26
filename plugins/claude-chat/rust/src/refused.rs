//! The side panel's Blocked: every action Claude Code refused in this chat, the chat's own and each subagent's. A subagent's
//! refusal shows only in its own transcript, so the chat reads it where it lands, in the tool's result, and keeps it on
//! the session that made the call (a subagent is a `Session` of its own). It only lists: the "Allow and
//! retry" button for the chat's own auto mode blocks stays in `blocked.rs`.
//!
//! Three kinds, matched on the start of an error result's text:
//! - auto mode's classifier said no: "Permission for this action was denied by the Claude Code auto mode classifier.
//!   Reason: [Undermining Oversight]. If you have other tasks ...".
//! - the classifier gave no verdict ("The server-side auto mode classifier gave no verdict for Bash: ..."): the call did not
//!   run and nobody judged it, listed as "Not reviewed".
//! - you answered no at a permission prompt ("The user doesn't want to proceed with this tool use. ...").

#[cfg(feature = "chat")]
use wings_plugin::ui::{Entry, Node, badge, entry, muted, stack, text};

#[cfg(feature = "chat")]
use crate::chat::{self, Chat};
use crate::session::{Detail, Session, Tool};

/// Who said no, and so how the row labels it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Auto mode's classifier refused it.
    Blocked,
    /// The classifier gave no verdict, so the call did not run.
    NotReviewed,
    /// You declined it at a permission prompt.
    Declined,
}

/// One refused call.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub tool_id: String,
    pub tool: String,
    /// The call's one line.
    pub summary: String,
    /// The call whole: the command, or what the line says.
    pub command: String,
    pub kind: Kind,
    /// Why, as a short sentence: "Undermining Oversight".
    pub reason: String,
}

const CLASSIFIER_DENIAL: &str = "Permission for this action was denied by the Claude Code auto mode classifier.";
const NO_VERDICT: &str = "The server-side auto mode classifier gave no verdict for ";
const DECLINED: &str = "The user doesn't want to proceed with this tool use.";

/// What a failed call's result says about a refusal: its kind and its reason, or None for any other failure.
pub fn read(output: &str) -> Option<(Kind, String)> {
    let output = output.trim_start();
    if let Some(rest) = output.strip_prefix(CLASSIFIER_DENIAL) {
        let said = rest.trim_start().strip_prefix("Reason:").map_or(rest, str::trim_start);
        let reason = said.split(". If you have other tasks").next().unwrap_or(said).trim().trim_end_matches('.');
        let reason = reason.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')).unwrap_or(reason);
        return Some((Kind::Blocked, if reason.is_empty() { "Blocked by auto mode".to_owned() } else { reason.to_owned() }));
    }
    if let Some(rest) = output.strip_prefix(NO_VERDICT) {
        let said = rest.split_once(": ").map_or(rest, |(_, said)| said);
        let reason = said.split(". This is not a judgment").next().unwrap_or(said).trim().trim_end_matches('.');
        return Some((Kind::NotReviewed, if reason.is_empty() { "Auto mode gave no verdict".to_owned() } else { reason.to_owned() }));
    }
    output.starts_with(DECLINED).then(|| (Kind::Declined, "You declined it".to_owned()))
}

/// The refusal `tool`'s failed result `output` says, when it says one.
pub fn of(tool: &Tool, output: &str) -> Option<Refusal> {
    let (kind, reason) = read(output)?;
    let command = match &tool.detail {
        Detail::Command(command) => command.clone(),
        _ => tool.summary.clone(),
    };
    Some(Refusal { tool_id: tool.id.clone(), tool: tool.name.clone(), summary: tool.summary.clone(), command, kind, reason })
}

/// Keeps `refusal`, unless the same call has one already.
pub fn keep(session: &mut Session, refusal: Refusal) {
    if !session.refusals.iter().any(|kept| kept.tool_id == refusal.tool_id) {
        session.refusals.push(refusal);
    }
}

/// A refusal with who made the call: `helper` is the subagent's description, None for this chat.
pub struct Listed<'a> {
    pub helper: Option<String>,
    pub refusal: &'a Refusal,
}

/// Every refusal of the chat and its subagents, this chat's first, then each subagent's in the order they started.
pub fn all(session: &Session) -> Vec<Listed<'_>> {
    let mut listed: Vec<Listed> = session.refusals.iter().map(|refusal| Listed { helper: None, refusal }).collect();
    for item in &session.items {
        let crate::session::Item::Tool(tool) = item else { continue };
        let Some(sub) = session.subagents.get(&tool.id) else { continue };
        let name = if tool.summary.is_empty() { "Subagent" } else { tool.summary.as_str() };
        listed.extend(sub.refusals.iter().map(|refusal| Listed { helper: Some(name.to_owned()), refusal }));
    }
    listed
}

/// How many refusals the subagent that `call` started hit.
pub fn of_subagent(session: &Session, call: &str) -> usize {
    session.subagents.get(call).map_or(0, |sub| sub.refusals.len())
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Blocked => "Blocked",
        Kind::NotReviewed => "Not reviewed",
        Kind::Declined => "Declined",
    }
}

#[cfg(feature = "chat")]
/// The row's key, which the open set holds it by.
fn id(refusal: &Refusal) -> String {
    format!("refused-{}", refusal.tool_id)
}

#[cfg(feature = "chat")]
/// The side panel's Blocked, while anything was refused: a row for each, with what was refused, who asked and why; a click
/// shows the whole command and the whole reason under it.
pub fn section(c: &Chat) -> Option<Node> {
    let listed = all(&c.session);
    if listed.is_empty() {
        return None;
    }
    let count = listed.len();
    let rows = listed.into_iter().map(|Listed { helper, refusal }| {
        let key = id(refusal);
        let opened = c.open_outputs.contains(&key);
        let line = format!("{} {}", refusal.tool, refusal.summary).trim().to_owned();
        let who = helper.as_deref().unwrap_or("This chat");
        let reason = if refusal.kind == Kind::Blocked { refusal.reason.clone() } else { format!("{} · {}", label(refusal.kind), refusal.reason) };
        let detail = muted(format!("{who} · {reason}")).prop("size", "small").prop("truncate", true).prop("title", format!("{who}: {reason}"));
        let toggle = key.clone();
        let head = entry(Entry {
            lead: Some(text("⚠").prop("tone", "warn")),
            title: text(line.clone()).prop("size", "small").prop("truncate", true).prop("title", line),
            detail: Some(detail),
            trailing: None,
        })
        .on("onClick", move |_| chat::toggle_output(&toggle))
        .prop("title", if opened { "Hide the whole command and reason" } else { "Show the whole command and reason" })
        .key(key.clone());
        let whole = opened.then(|| {
            let command = (!refusal.command.is_empty()).then(|| text(refusal.command.clone()).prop("mono", true).prop("size", "small"));
            let said = text(refusal.reason.clone()).prop("size", "small").prop("tone", "muted");
            crate::view::inset(stack().prop("gap", "xs").maybe(command).child(said)).key(format!("{key}-whole"))
        });
        stack().child(head).maybe(whole).key(format!("{key}-row"))
    });
    Some(crate::view::fold_list(c, "refused", format!("Blocked · {count}"), rows))
}

#[cfg(feature = "chat")]
/// The warning a subagent's row carries when it hit refusals: "1 blocked".
pub fn badge_for(c: &Chat, call: &str) -> Option<Node> {
    let count = of_subagent(&c.session, call);
    (count > 0).then(|| badge(format!("{count} blocked"), Some("warn")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AUTO: &str = "Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Undermining Oversight]. If you have other tasks that don't depend on this action, continue working on those. IMPORTANT: You *may* attempt to accomplish this action using other tools";

    fn call(session: &mut Session, parent: Option<&str>, id: &str, command: &str, result: &str, is_error: bool) {
        let parent = parent.map_or(json!(null), |parent| json!(parent));
        session.hear(&json!({ "type": "assistant", "parent_tool_use_id": parent, "message": { "content": [{ "type": "tool_use", "id": id, "name": "Bash", "input": { "command": command } }] } }));
        session.hear(&json!({ "type": "user", "parent_tool_use_id": parent, "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "is_error": is_error, "content": result }] } }));
    }

    fn agent(session: &mut Session) {
        session.hear(&json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_use", "id": "agent1", "name": "Agent", "input": { "description": "Probe Vaultwarden", "prompt": "go" } }] } }));
    }

    #[test]
    fn the_three_refusals_are_read_and_a_plain_failure_is_not() {
        assert_eq!(read(AUTO), Some((Kind::Blocked, "Undermining Oversight".into())));
        let free = "Permission for this action was denied by the Claude Code auto mode classifier. Reason: The server-side auto mode classifier judged this action dangerous (it gave no explanation). If you have other tasks";
        assert_eq!(read(free), Some((Kind::Blocked, "The server-side auto mode classifier judged this action dangerous (it gave no explanation)".into())));
        let none = "The server-side auto mode classifier gave no verdict for Bash: another call in this response was changed after auto mode reviewed it, so this call's review no longer applies. This is not a judgment that the action is unsafe. Issue it again";
        assert_eq!(read(none), Some((Kind::NotReviewed, "another call in this response was changed after auto mode reviewed it, so this call's review no longer applies".into())));
        let no = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP";
        assert_eq!(read(no), Some((Kind::Declined, "You declined it".into())));
        assert_eq!(read("Exit code 1\nls: nope: No such file or directory"), None);
        assert_eq!(read("Error: File does not exist."), None);
    }

    #[test]
    fn a_refusal_in_this_chat_is_listed_as_this_chat() {
        let mut session = Session::default();
        call(&mut session, None, "t1", "git push origin main", AUTO, true);
        let listed = all(&session);
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].helper.clone(), listed[0].refusal.reason.as_str(), listed[0].refusal.command.as_str()), (None, "Undermining Oversight", "git push origin main"));
        assert_eq!(listed[0].refusal.tool, "Bash");
    }

    #[test]
    fn a_subagents_refusal_is_listed_under_its_description_and_counted_on_its_row() {
        let mut session = Session::default();
        agent(&mut session);
        call(&mut session, Some("agent1"), "s1", "container run -d --rm --name bw-probe-vw x", AUTO, true);
        let listed = all(&session);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].helper.as_deref(), Some("Probe Vaultwarden"));
        assert_eq!(listed[0].refusal.reason, "Undermining Oversight");
        assert_eq!(of_subagent(&session, "agent1"), 1);
        // The same result heard again is one refusal.
        call(&mut session, Some("agent1"), "s1", "container run -d --rm --name bw-probe-vw x", AUTO, true);
        assert_eq!(all(&session).len(), 1);
    }

    #[test]
    fn a_call_that_failed_for_any_other_reason_is_not_listed() {
        let mut session = Session::default();
        agent(&mut session);
        call(&mut session, None, "t1", "false", "Exit code 1", true);
        call(&mut session, Some("agent1"), "s1", "ls nope", "ls: nope: No such file or directory", true);
        call(&mut session, None, "t2", "echo hi", AUTO, false);
        assert!(all(&session).is_empty());
        assert_eq!(of_subagent(&session, "agent1"), 0);
    }

    #[test]
    fn no_verdict_and_declined_are_listed_with_their_label() {
        let mut session = Session::default();
        call(&mut session, None, "t1", "rtk git status", "The server-side auto mode classifier gave no verdict for Bash: a hook changed this call's input after auto mode reviewed it. This is not a judgment that the action is unsafe.", true);
        call(&mut session, None, "t2", "rm -r x", "The user doesn't want to proceed with this tool use. The tool use was rejected", true);
        let kinds: Vec<Kind> = all(&session).iter().map(|listed| listed.refusal.kind).collect();
        assert_eq!(kinds, [Kind::NotReviewed, Kind::Declined]);
    }
}
