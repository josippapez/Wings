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

/// "Continue when it resets": sleeps until then, once. Wings lets the message go then without another click only
/// because this click armed it (`processArm`), so it is armed with Wings here, in the click.
pub fn arm() {
    let armed = chat::with(|c| {
        let resets = c.session.limit.resets_at(&c.session.limits)?;
        let (generation, delay) = c.session.limit.arm(resets, host::now())?;
        Some((generation, delay, resets, c.input.clone()))
    });
    let Some((generation, delay, resets, input)) = armed else {
        return;
    };
    if let Some(input) = input {
        let held = input.arm(Some(limit_wait::CONTINUE), resets);
        task::spawn(async move {
            // A Wings that won't hold it would refuse the message at the reset: the card offers the button again.
            if held.await.is_err() {
                chat::with(|c| {
                    if c.session.limit.generation == generation {
                        c.session.limit.unarm();
                    }
                });
                chat::save_tab();
            }
        });
    }
    sleep(generation, delay);
    chat::save_tab();
}

/// Takes the wait back, and keeps the card.
fn unarm() {
    let input = chat::with(|c| {
        c.session.limit.unarm();
        c.input.clone()
    });
    if let Some(input) = input {
        drop(input.arm(None, 0.0));
    }
    chat::save_tab();
}

/// "Not now".
fn dismiss() {
    chat::with(|c| c.session.limit.dismiss());
    chat::save_tab();
}

/// The card again after a start of the tab, for the reset it kept (`Chat::limit_resume`). Called as the chat begins. It
/// is not armed again: what your click armed with Wings is gone with the Wings that quit, so the card asks the click
/// again ("Continue when it resets", or "Continue" once it has reset).
pub(crate) fn resume(c: &mut Chat) {
    let Some(resets) = c.limit_resume.take() else {
        return;
    };
    c.session.limit.offer(resets, host::now());
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

/// Sends `said` as your message. When Claude Code doesn't get it, the chat says so and isn't left working on it.
fn say(said: &'static str) {
    let written = chat::with(|c| {
        let input = c.input.clone()?;
        c.requests += 1;
        let line = c.session.send(said, &[], host::now() as u64, c.requests);
        c.doing = (c.session.activity(), host::now() as u64);
        Some((input, line))
    });
    if let Some((input, line)) = written {
        let wrote = input.write(&line);
        task::spawn(async move {
            if let Err(error) = wrote.await {
                chat::with(|c| {
                    c.session.working = false;
                    c.session.items.push(crate::session::Item::Error(format!("Claude Code didn't get the message to continue: {error}")));
                });
                chat::report_agent();
            }
        });
        crate::reattach::sync();
    }
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
    fn a_reset_the_tab_kept_offers_the_button_again_as_the_chat_begins() {
        let calls = wings();
        with(|c| {
            c.limit_resume = Some(RESET);
            chat::begin(c);
            assert_eq!(c.session.limit.armed, None, "what was armed with Wings is gone with it");
            assert_eq!(c.limit_resume, None);
            assert!(card(c).is_some());
        });
        // "Not now" drops the card, and the tab keeps no arm.
        dismiss();
        let saved = calls.borrow().iter().rev().find(|(method, _)| method == "setTabState").map(|(_, params)| params["state"]["limitArmed"].clone());
        assert_eq!(saved, Some(Value::Null));
    }

    /// The click arms the message with Wings, which lets it go at the reset without another click; Cancel takes it back.
    #[test]
    fn the_click_arms_the_continue_with_wings_and_cancel_takes_it_back() {
        let calls = limited();
        arm();
        task::block_on(async {});
        let arms: Vec<Value> = calls.borrow().iter().filter(|(method, _)| method == "processArm").map(|(_, params)| params.clone()).collect();
        assert_eq!(arms.len(), 1);
        assert_eq!((arms[0]["text"].as_str(), arms[0]["at"].as_f64()), (Some(limit_wait::CONTINUE), Some(RESET)));
        unarm();
        let last = calls.borrow().iter().rev().find(|(method, _)| method == "processArm").map(|(_, params)| params["text"].clone());
        assert_eq!(last, Some(Value::Null));
    }

    /// A Wings that won't hold the message (refused, or one before 0.2.93) leaves the card offering the button again.
    #[test]
    fn a_refused_arm_leaves_the_card_offering_the_button() {
        let _ = limited();
        host::testing::answer(|method, _| if method == "processArm" { Err("A page can write to a program only right after you click or type in it".into()) } else { Ok(Value::Null) });
        arm();
        task::block_on(async {});
        assert_eq!(with(|c| c.session.limit.armed), None);
    }

    /// The message that didn't reach Claude Code says so, and the chat isn't left working on it.
    #[test]
    fn a_continue_claude_code_never_got_leaves_the_chat_idle_with_why() {
        let _ = limited();
        arm();
        let generation = with(|c| c.session.limit.generation);
        host::testing::answer(|method, _| if method == "processWrite" { Err("A page can write to a program only right after you click or type in it".into()) } else { Ok(Value::Null) });
        fire(generation);
        task::block_on(async {});
        with(|c| {
            assert!(!c.session.working);
            assert!(c.session.items.iter().any(|item| matches!(item, Item::Error(said) if said.contains("didn't get the message to continue"))));
        });
    }
}
