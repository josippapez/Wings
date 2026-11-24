//! Plans as documents (`plan_doc.rs`): a "Plan" group in the side panel that lists every plan Claude proposed, newest
//! first, a link on the plan's card and on its row in the conversation, and the page a plan opens as. The page replaces the
//! conversation, as a breadcrumb back says: the plan as Markdown in a reading column, a speech bubble beside each part to
//! comment on it, your comments under it, and the buttons that answer the plan or send the comments.

use wings_plugin::{
    format::plural,
    host,
    markdown::markdown,
    ui::{Entry, Node, badge, button, card, empty, entry, icon, muted, notice, page, row, select, stack, text, text_input, toolbar_with},
};

use crate::{
    chat::{self, Chat},
    plan_doc::{self, Block, Plan, Status},
};

/// The reading column's width.
const COLUMN: u32 = 860;
/// A speech bubble, in a 24-point box.
const BUBBLE: [&str; 3] = ["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z", "M12 7v6", "M9 10h6"];
const CLOSE: &str = "M3.5 3.5l7 7M10.5 3.5l-7 7";

fn tone(status: Status) -> Option<&'static str> {
    match status {
        Status::Waiting => Some("warn"),
        Status::Approved => Some("success"),
        Status::Rejected => Some("danger"),
        Status::Unknown => None,
    }
}

// ---------- the side panel and the links ----------

/// The side panel's group of plans: the newest few by their first line, each with how it stands and how many comments wait
/// on it, a click opening it. None while Claude has proposed none.
pub fn section(c: &Chat) -> Option<Node> {
    let plans = c.session.plans.shown(&c.session.items);
    if plans.is_empty() {
        return None;
    }
    let title = if plans.len() == 1 { "Plan".to_owned() } else { format!("Plans · {}", plans.len()) };
    let rows = plans.iter().map(|(number, plan)| {
        let id = plan.id.clone();
        let comments = c.review.iter().filter(|comment| comment.path == plan_doc::path(*number)).count();
        let mut said = plan_doc::status(&c.session.items, plan).label().to_owned();
        if comments > 0 {
            said.push_str(&format!(" · {}", plural(comments, "comment")));
        }
        let name = plan_doc::title(plan);
        entry(Entry {
            lead: Some(crate::view::lead(muted(format!("{number}")).prop("size", "small"))),
            title: text(name.as_str()).prop("size", "small").prop("clamp", true).prop("title", name.as_str()),
            detail: Some(muted(said).prop("size", "small")),
            trailing: None,
        })
        .on("onClick", move |_| open(&id))
        .prop("title", "Open the plan as a document")
        .key(plan.id.as_str())
    });
    Some(crate::view::fold_list(c, "plans", title, rows))
}

/// The card's link, for the plan whose question shows `text`.
pub fn card_link(c: &Chat, text: &str) -> Option<Node> {
    let plan = c.session.plans.shown(&c.session.items).into_iter().find(|(_, plan)| plan.text == text)?.1;
    let id = plan.id.clone();
    Some(button("Open as document", "ghost", false, move || open(&id)).prop("title", "Read the plan as a page and comment on its parts"))
}

/// The link on the conversation's row for an ExitPlanMode call `tool`.
pub fn step_link(c: &Chat, tool: &crate::session::Tool) -> Option<Node> {
    if tool.name != "ExitPlanMode" || !c.session.plans.shown(&c.session.items).iter().any(|(_, plan)| plan.id == tool.id) {
        return None;
    }
    let id = tool.id.clone();
    Some(button("Open plan", "ghost", false, move || open(&id)).prop("size", "sm").prop("title", "Read the plan as a page and comment on its parts"))
}

/// The label of the card's button that sends the plan back: it says when comments go with it.
pub fn revise_label(c: &Chat) -> String {
    match plan_doc::count(&c.review) {
        0 => "Keep planning".to_owned(),
        n => format!("Revise with {}", plural(n, "comment")),
    }
}

// ---------- what a click does ----------

pub fn open(id: &str) {
    chat::with(|c| {
        c.plan_view.open = Some(id.to_owned());
        c.plan_view.editing = None;
        c.plan_view.draft.clear();
    });
}

fn close() {
    chat::with(|c| c.plan_view.open = None);
}

fn write_on(start: u64) {
    chat::with(|c| {
        c.plan_view.editing = Some(start);
        c.plan_view.draft.clear();
    });
}

fn stop_writing() {
    chat::with(|c| c.plan_view.editing = None);
}

fn typed(said: String) {
    chat::with(|c| c.plan_view.draft = said);
}

/// Keeps what was typed as a comment on the block of `id`'s plan that starts at `start`.
fn add(id: &str, start: u64) {
    let added = chat::with(|c| {
        let (number, text) = c.session.plans.shown(&c.session.items).into_iter().find(|(_, plan)| plan.id == id).map(|(number, plan)| (number, plan.text.clone()))?;
        let block = plan_doc::blocks(&text).into_iter().find(|block| block.start == start)?;
        let comment = plan_doc::comment(number, &block, &c.plan_view.draft, host::now() as u64)?;
        if !crate::review::fits(&c.review, &comment) {
            c.review_note = Some(crate::review::FULL.to_owned());
            return None;
        }
        c.review_note = None;
        c.review.push(comment);
        c.plan_view.editing = None;
        c.plan_view.draft.clear();
        Some(())
    });
    if added.is_some() {
        chat::save_tab();
    }
}

fn remove(comment: &str) {
    chat::with(|c| c.review.retain(|had| had.id != comment));
    chat::save_tab();
}

/// Sends the comments with your next message, which the message box holds the words of: back to the conversation, where it
/// shows.
fn send_comments() {
    close();
    chat::send();
}

/// Answers the plan with the comments, back in the conversation.
fn revise(request: &str) {
    close();
    chat::keep_planning(request);
}

fn approve(request: &str, mode: &str) {
    close();
    chat::approve_plan(request, mode);
}

fn refresh() {
    chat::write(|c, _| c.session.plans.refresh_line());
}

// ---------- the page ----------

fn back() -> Node {
    button("← Chat", "ghost", false, close).prop("title", "Back to the conversation")
}

/// The plan page while one is open, else None.
pub fn page_of(c: &Chat) -> Option<Node> {
    let open = c.plan_view.open.as_deref()?;
    let plans = c.session.plans.shown(&c.session.items);
    let Some(&(number, plan)) = plans.iter().find(|(_, plan)| plan.id == open) else {
        return Some(page(
            toolbar_with("Plan", back(), None),
            [empty("This plan is gone", "A rewind went back to before Claude proposed it.")],
        ));
    };
    let status = plan_doc::status(&c.session.items, plan);
    let waiting = plan_doc::waiting(&c.session.items, plan).map(str::to_owned);
    let mine = plan_doc::path(number);
    let pending = c.review.iter().filter(|comment| comment.path == mine).count();
    let newest = plans.first().is_some_and(|(_, newest)| newest.id == plan.id);

    // Only the way back: the plan's name is the page's heading below, and in the bar beside three buttons it was cut to "…".
    let trail = back();
    let actions = match waiting {
        Some(request) => {
            let (revise_request, approve_request, auto_request) = (request.clone(), request.clone(), request);
            let label = if pending > 0 { format!("Revise with {}", plural(pending, "comment")) } else { "Keep planning".to_owned() };
            row()
                .prop("gap", "s")
                .prop("wrap", true)
                .child(button(label, "outline", false, move || revise(&revise_request)))
                .child(button("Approve", "secondary", false, move || approve(&approve_request, "default")))
                .child(button("Approve and auto-accept edits", "default", false, move || approve(&auto_request, "acceptEdits")))
        }
        None => {
            let all = plan_doc::count(&c.review);
            row()
                .prop("gap", "s")
                .prop("wrap", true)
                .maybe(newest.then(|| button("Refresh", "ghost", false, refresh).prop("title", "Read the plan file again from Claude Code").prop("disabled", c.session.plans.refreshing || c.input.is_none())))
                .maybe((all > 0).then(|| button(format!("Send {}", plural(all, "comment")), "default", false, send_comments)))
        }
    };

    let mut column = stack().prop("gap", "m").prop("maxWidth", COLUMN).child(meta(c, plans.as_slice(), number, plan, status));
    if let Some(why) = &c.session.plans.note {
        column = column.child(notice("info", why.as_str()));
    }
    // The nodes of the plan stop short of the app's limit for a page, as the conversation's do.
    let mut used = column.count();
    let all = plan_doc::blocks(&plan.text);
    let mut cut = false;
    for block in &all {
        let part = block_node(c, number, &plan.id, block);
        used += part.count();
        if used > crate::view::BUDGET / 2 {
            cut = true;
            break;
        }
        column = column.child(part);
    }
    if cut {
        column = column.child(muted("The rest of this plan is too long to show here.").prop("size", "small"));
    }
    Some(page(toolbar_with("Plan", trail, Some(actions)), [Node::new("Scroll").prop("textScale", c.text_scale as f64).child(column)]))
}

/// Above the plan: how it stands, which of the plans it is with a picker for the others, and what the bubbles are for.
fn meta(c: &Chat, plans: &[(usize, &Plan)], number: usize, plan: &Plan, status: Status) -> Node {
    let picker = (plans.len() > 1).then(|| {
        let options: Vec<(String, String)> = plans.iter().map(|(n, other)| (other.id.clone(), format!("Plan {n} · {}", plan_doc::title(other)))).collect();
        select("Plan", &options, &plan.id, |id| open(id)).prop("variant", "ghost").key("plan-picker")
    });
    let status = match tone(status) {
        Some(tone) => badge(status.label(), Some(tone)),
        None => badge(status.label(), None),
    };
    let hint = if plan_doc::count(&c.review) == 0 {
        "Press a speech bubble to comment on a part. Comments go with your next message."
    } else {
        "Comments go with your next message, or with Revise while the plan waits."
    };
    stack()
        .prop("gap", "xs")
        .child(row().prop("gap", "s").prop("align", "center").prop("wrap", true).child(status).child(muted(format!("Plan {number} of {}", plans.len()))).maybe(picker))
        .child(muted(hint).prop("size", "small"))
}

/// One part of the plan: its Markdown with the speech bubble beside it, the comments you left on it under it, and the field
/// for the one being written.
fn block_node(c: &Chat, number: usize, id: &str, block: &Block) -> Node {
    let start = block.start;
    let place = if block.end > start { format!("lines {start} to {}", block.end) } else { format!("line {start}") };
    let bubble = button("", "ghost", false, move || write_on(start))
        .prop("size", "icon-sm")
        .prop("label", format!("Comment on {place}"))
        .prop("title", format!("Comment on {place}"))
        .child(icon(24, 2.0, &BUBBLE).prop("size", 14));
    let mine = plan_doc::path(number);
    let comments = c.review.iter().filter(|comment| comment.path == mine && comment.line == start).map(|comment| {
        let gone = comment.id.clone();
        card().key(comment.id.as_str()).child(
            row()
                .prop("gap", "s")
                .prop("align", "center")
                .child(text(comment.body.as_str()).prop("size", "small").prop("grow", true))
                .child(
                    button("", "ghost", false, move || remove(&gone))
                        .prop("size", "icon-sm")
                        .prop("label", "Remove the comment")
                        .prop("title", "Remove the comment")
                        .child(icon(12, 2.0, &[CLOSE])),
                ),
        )
    });
    let writing = (c.plan_view.editing == Some(start)).then(|| {
        let to = id.to_owned();
        let blank = c.plan_view.draft.trim().is_empty();
        stack()
            .prop("gap", "s")
            .child(text_input(&format!("Comment on {place}"), c.plan_view.draft.as_str(), typed).prop("placeholder", "What to change here").prop("autoFocus", true))
            .child(
                row()
                    .prop("gap", "s")
                    .prop("justify", "end")
                    .child(button("Cancel", "ghost", false, stop_writing))
                    .child(button("Add comment", "default", false, move || add(&to, start)).prop("disabled", blank)),
            )
            .key("writing")
    });
    stack()
        .prop("gap", "xs")
        .child(row().prop("gap", "s").child(stack().prop("grow", true).child(markdown(&block.source()))).child(bubble))
        .children(comments)
        .maybe(writing)
        .key(format!("block-{start}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_says_how_many_comments_go_with_keep_planning() {
        chat::with(|c| c.review.clear());
        assert_eq!(chat::with(|c| revise_label(c)), "Keep planning");
        let block = Block { start: 2, end: 2, text: "- x".into() };
        chat::with(|c| c.review.push(plan_doc::comment(1, &block, "Why?", 1).unwrap()));
        assert_eq!(chat::with(|c| revise_label(c)), "Revise with 1 comment");
    }

    #[test]
    fn an_added_comment_is_kept_on_the_block_and_goes_with_the_reply_that_keeps_planning() {
        wings_plugin::host::testing::answer(|_, _| Ok(serde_json::Value::Null));
        let line = r##"{"type":"control_request","request_id":"p1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","tool_use_id":"t1","input":{"plan":"# Plan\n\n1. Move it\n2. Test it"}}}"##;
        chat::with(|c| {
            c.review.clear();
            c.session.hear(&serde_json::from_str(line).unwrap());
        });
        let (open_id, shown) = chat::with(|c| {
            let shown = c.session.plans.shown(&c.session.items);
            (shown[0].1.id.clone(), shown.len())
        });
        assert_eq!((open_id.as_str(), shown), ("t1", 1));
        // The question is the only thing that names the plan here, so it is shown, waiting.
        open(&open_id);
        write_on(4);
        typed("Which tests?".into());
        add(&open_id, 4);
        let kept = chat::with(|c| (c.review.len(), c.review[0].place(), c.review[0].quote.clone(), c.plan_view.editing));
        assert_eq!(kept, (1, "plan #1:4".to_owned(), "2. Test it".to_owned(), None));
        let node = chat::with(|c| page_of(c)).expect("the plan page");
        assert!(node.count() > 10);
        chat::with(|c| {
            let said = crate::review::message("", &plan_doc::take(&mut c.review));
            assert!(said.contains("plan #1:4\n```\n2. Test it\n```\nWhich tests?"));
            c.plan_view.open = None;
            c.session.items.clear();
        });
    }
}
