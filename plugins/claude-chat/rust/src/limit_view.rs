//! The card for a hit usage limit (`limit_wait.rs`) at the foot of the conversation, and what its buttons do: arm one sleep
//! until the reset, after which one message goes if the chat is still open and idle.

use wings_plugin::{
    host, task,
    ui::{Node, button, confirm, muted, row, text},
};

use crate::{
    chat::{self, Chat},
    limit_wait::{self, Phase},
};

/// The card, while a limit is hit and Claude isn't working on something else.
pub fn card(c: &Chat) -> Option<Node> {
    let now = host::now();
    let card = c.session.limit.card(now, &c.session.limits)?;
    if c.session.working && card.phase != Phase::Armed {
        return None;
    }
    let (head, small) = limit_wait::lines(&card, now);
    let body = [text(head)].into_iter().chain(small.map(|small| muted(small).prop("size", "small")));
    let not_now = button("Not now", "ghost", false, dismiss);
    let actions = row().prop("grow", true).prop("gap", "s").prop("justify", "end");
    let actions = match card.phase {
        Phase::Offer => actions.child(not_now).child(button("Continue when it resets", "default", false, arm)),
        Phase::Armed => actions.child(button("Cancel", "outline", false, unarm)),
        Phase::Passed => actions.child(not_now).child(button("Continue", "default", false, send_now)),
        Phase::Far | Phase::Unknown | Phase::Skipped(_) => actions.child(not_now),
    };
    Some(confirm("Usage limit reached", body, [actions]).key("limit-wait"))
}

/// "Continue when it resets": sleeps until then, once.
pub fn arm() {
    let armed = chat::with(|c| {
        let resets = c.session.limit.resets_at(&c.session.limits)?;
        c.session.limit.arm(resets, host::now())
    });
    if let Some((generation, delay)) = armed {
        sleep(generation, delay);
        chat::save_tab();
    }
}

/// Takes the wait back, and keeps the card.
fn unarm() {
    chat::with(|c| c.session.limit.unarm());
    chat::save_tab();
}

/// "Not now".
fn dismiss() {
    chat::with(|c| c.session.limit.dismiss());
    chat::save_tab();
}

/// Arms again after a start of the tab, for the reset it kept (`Chat::limit_resume`). Called as the chat begins.
pub(crate) fn resume(c: &mut Chat) {
    let Some(resets) = c.limit_resume.take() else {
        return;
    };
    if let Some((generation, delay)) = c.session.limit.rearm(resets, host::now()) {
        sleep(generation, delay);
    }
}

/// The one sleep of an arm: the host's timer, not a poll.
fn sleep(generation: u32, delay: u32) {
    task::spawn(async move {
        host::sleep(delay).await;
        fire(generation);
    });
}

/// The reset came: sends the message when `LimitWait::fire` says to.
pub(crate) fn fire(generation: u32) {
    let send = chat::with(|c| {
        let (open, idle, empty) = (c.input.is_some(), !c.session.working, c.draft.trim().is_empty() && c.images.is_empty());
        c.session.limit.fire(generation, open, idle, empty)
    });
    if let Some(said) = send {
        say(said);
    }
    chat::save_tab();
}

/// "Continue", once it has reset: the terminal's own message, now.
fn send_now() {
    say(limit_wait::CONTINUE);
}

fn say(said: &'static str) {
    chat::write(|c, n| {
        let line = c.session.send(said, &[], host::now() as u64, n);
        c.doing = (c.session.activity(), host::now() as u64);
        Some(line)
    });
    chat::report_agent();
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wings_plugin::{host, task};

    use crate::{chat::with, session::Item};

    use super::*;

    const RESET: f64 = 1_795_000_000_000.0 + 3_600_000.0;

    /// Wings answering every call, keeping them.
    fn wings() -> std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>> {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(Value::Null)
        });
        host::testing::set_now(1_795_000_000_000.0);
        calls
    }

    /// A chat that runs Claude Code, with the limit hit by `rate_limit_event` and the turn's result.
    fn limited() -> std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>> {
        let calls = wings();
        let process = host::spawn("claude", &[], "/work/shop", "", |_| {});
        with(|c| {
            c.session = crate::session::Session::default();
            c.draft.clear();
            c.input = Some(process.input());
            c.session.send("go on", &[], 1, 1);
        });
        let said = "You've hit your 5-hour limit · resets 4:00pm";
        for line in [
            json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": RESET / 1000.0, "rateLimitType": "five_hour" } }),
            json!({ "type": "result", "subtype": "success", "is_error": true, "result": said }),
        ] {
            with(|c| c.session.hear(&line));
        }
        calls
    }

    fn writes(calls: &std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>>) -> Vec<String> {
        task::block_on(async {});
        calls.borrow().iter().filter(|(method, _)| method == "processWrite").map(|(_, params)| params["text"].as_str().unwrap().to_owned()).collect()
    }

    fn continues(writes: &[String]) -> usize {
        writes.iter().filter(|write| write.contains(limit_wait::CONTINUE)).count()
    }

    #[test]
    fn the_card_shows_for_the_hit_limit_and_not_while_claude_works() {
        let _ = limited();
        assert!(with(|c| card(c).is_some()));
        with(|c| c.session.send("again", &[], 2, 2));
        assert!(with(|c| card(c).is_none()), "a message of yours ends the wait");
    }

    #[test]
    fn arming_and_the_reset_send_exactly_one_continue() {
        let calls = limited();
        arm();
        let generation = with(|c| {
            assert_eq!(c.session.limit.armed, Some(RESET));
            c.session.limit.generation
        });
        fire(generation);
        fire(generation);
        let sent = writes(&calls);
        assert_eq!(continues(&sent), 1, "{sent:?}");
        assert!(with(|c| c.session.items.iter().any(|item| matches!(item, Item::User(said) if said == limit_wait::CONTINUE)) && c.session.working));
    }

    #[test]
    fn a_message_typed_or_sent_meanwhile_or_a_closed_chat_sends_nothing() {
        // Sent: the arm was dropped.
        let calls = limited();
        arm();
        let generation = with(|c| c.session.limit.generation);
        with(|c| c.session.send("never mind", &[], 3, 3));
        fire(generation);
        assert_eq!(continues(&writes(&calls)), 0);
        // Typed, not sent.
        let calls = limited();
        arm();
        let generation = with(|c| {
            c.draft = "half a thought".into();
            c.session.limit.generation
        });
        fire(generation);
        assert_eq!(continues(&writes(&calls)), 0);
        with(|c| c.draft.clear());
        // The chat's Claude Code is gone.
        let calls = limited();
        arm();
        let generation = with(|c| {
            c.input = None;
            c.session.limit.generation
        });
        fire(generation);
        assert_eq!(continues(&writes(&calls)), 0);
    }

    #[test]
    fn a_reset_the_tab_kept_arms_again_as_the_chat_begins_and_the_tab_keeps_the_arm() {
        let calls = wings();
        with(|c| {
            c.limit_resume = Some(RESET);
            chat::begin(c);
            assert_eq!(c.session.limit.armed, Some(RESET));
            assert_eq!(c.limit_resume, None);
        });
        chat::save_tab();
        let saved = calls.borrow().iter().rev().find(|(method, _)| method == "setTabState").map(|(_, params)| params["state"]["limitArmed"].clone());
        assert_eq!(saved, Some(json!(RESET)));
        // "Not now" drops it, and so does the tab's state.
        dismiss();
        let saved = calls.borrow().iter().rev().find(|(method, _)| method == "setTabState").map(|(_, params)| params["state"]["limitArmed"].clone());
        assert_eq!(saved, Some(Value::Null));
    }
}
