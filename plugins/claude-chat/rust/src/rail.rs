//! The rail at the left of the conversation: one tick for each message you sent (queued ones aren't in the conversation
//! yet, so they have none), the tick of the prompt you are reading long, a card on hover with the prompt's first line and
//! the start of Claude's reply, and a click that scrolls the conversation so that prompt is at its top.
//!
//! The page sends one `PromptRail` node (Wings 0.2.44) with each prompt's words. Wings draws the ticks, the fisheye and
//! the hover card itself, so hovering never reaches the page, and a long session costs one node, not one for each prompt.
//!
//! How it scrolls and knows where you are: the conversation's `Scroll` takes `scrollTo` (the child of its column to bring
//! to the top, once for each token) and `track` with `onTrack` (which prompt's child the view has reached), both Wings
//! 0.2.42. The page tells Wings only the children's numbers; Wings reads their tops while it draws and tells the page
//! only when the prompt in view changes, so reading a long answer costs the page nothing.

use std::cell::RefCell;

use serde_json::{Value, json};
use wings_plugin::{format::truncate, ui::Node};

use crate::{
    chat::{self, Chat},
    session::Item,
    view::COLUMN,
};

/// The most prompts sent to the rail, the newest: every render of the page carries them, and Wings takes 2,000.
pub const MARKS: usize = 500;
/// The longest first line, and the longest start of a reply, the card shows.
const LINE: usize = 200;
const REPLY: usize = 300;

#[derive(Default)]
struct State {
    /// The prompt (as its item's index) a click asked for, until it is scrolled to or can't be.
    jump: Option<usize>,
    /// Each click has the next one, so Wings scrolls once for each.
    token: u64,
    /// What Wings last said: none before it has said, then the prompt the view has reached, as its item's index, or none
    /// above them all. Kept by item rather than by its place among those drawn, which moves as older ones load.
    at: Option<Option<usize>>,
    /// "Jump to the latest" asked for the end of the conversation, with `token`.
    end: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// What Wings says of the tracked prompts: the last whose top the view has reached, as its item's index.
pub fn tracked(item: Option<usize>) {
    STATE.with(|state| state.borrow_mut().at = Some(item));
}

/// What Wings last said of the view, for `Rail::of`.
pub fn state_at() -> Option<Option<usize>> {
    STATE.with(|state| state.borrow().at)
}

/// The messages you sent, by their item's index in the conversation.
pub fn prompt_items(items: &[Item]) -> Vec<usize> {
    items.iter().enumerate().filter(|(_, item)| matches!(item, Item::User(_) | Item::UserImages(..))).map(|(at, _)| at).collect()
}

/// One message you sent.
#[derive(Debug, PartialEq)]
pub struct Prompt {
    /// Its item's index in the conversation.
    pub item: usize,
    /// Its place among the children of the conversation's column, when the page has built it.
    pub child: Option<usize>,
}

/// The prompts of a conversation and the one in view. `built` is the (item, child) of each prompt the page drew, oldest
/// first, and `at` what Wings last said of the view (`tracked`).
#[derive(Debug, PartialEq)]
pub struct Rail {
    pub prompts: Vec<Prompt>,
    /// The index among `prompts` of the one being read.
    pub current: usize,
}

impl Rail {
    /// `at` is what Wings last said (`tracked`).
    pub fn of(items: &[Item], built: &[(usize, usize)], at: Option<Option<usize>>) -> Rail {
        let prompts: Vec<Prompt> = prompt_items(items)
            .into_iter()
            .map(|item| Prompt { item, child: built.binary_search_by_key(&item, |(built, _)| *built).ok().map(|at| built[at].1) })
            .collect();
        let first = prompts.iter().position(|prompt| prompt.child.is_some());
        let current = match (at, first) {
            // The prompt the view reached, or one before it where it is no longer a prompt, as after a rewind.
            (Some(Some(item)), _) => prompts.iter().rposition(|prompt| prompt.item <= item).unwrap_or(0),
            (Some(None), Some(first)) => first,
            // Before Wings has said, the view is at the end of the conversation.
            _ => prompts.len().saturating_sub(1),
        };
        Rail { prompts, current }
    }

    /// The children Wings is asked to track: each drawn prompt's, oldest first.
    pub fn track(&self) -> Vec<usize> {
        self.prompts.iter().filter_map(|prompt| prompt.child).collect()
    }
}

/// A click on a tick: the conversation scrolls so `item`, a message of yours, is at its top. A prompt outside what the
/// page shows is brought in first.
pub fn go(item: usize) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.token += 1;
        state.jump = Some(item);
        state.end = false;
    });
    chat::with(|c| crate::view::show_item(c, item));
}

/// The conversation scrolls to its last child, once the page shows the newest items.
pub fn to_end() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.token += 1;
        state.jump = None;
        state.end = true;
    });
}

/// What the conversation's `Scroll` is told after a click: the child to bring to the top, with the click's token. The page
/// keeps saying it, which Wings acts on once. A prompt the page couldn't draw (it is past what the page may send) is given
/// up on, so it isn't scrolled to later, when scrolling up draws it.
pub fn scroll_to(rail: &Rail, last: usize) -> Option<Value> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.end {
            return Some(json!({ "index": last, "token": state.token }));
        }
        let item = state.jump?;
        match rail.prompts.iter().find(|prompt| prompt.item == item).and_then(|prompt| prompt.child) {
            Some(index) => Some(json!({ "index": index, "token": state.token })),
            None => {
                state.jump = None;
                None
            }
        }
    })
}

/// What the page says of the rail to the conversation's `Scroll`, whose column's last child is `last`: its props, and
/// `None` while there are no prompts.
pub fn watch(scroll: Node, rail: &Rail, last: usize) -> Node {
    if rail.prompts.is_empty() {
        return scroll;
    }
    // Wings says a place among the tracked children, which is kept as the prompt there.
    let drawn: Vec<usize> = rail.prompts.iter().filter(|prompt| prompt.child.is_some()).map(|prompt| prompt.item).collect();
    let scroll = scroll.prop("track", json!(rail.track())).on("onTrack", move |payload| {
        tracked(payload["at"].as_u64().map(|at| drawn.get(at as usize).or(drawn.last()).copied().unwrap_or_default()));
    });
    match scroll_to(rail, last) {
        Some(to) => scroll.prop("scrollTo", to),
        None => scroll,
    }
}

/// The first line of a message, as the card says it.
fn first_line(item: &Item) -> String {
    let (Item::User(said) | Item::UserImages(said, _)) = item else {
        return String::new();
    };
    match said.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => truncate(line, LINE),
        None => "Picture".to_owned(),
    }
}

/// The start of what Claude first wrote in answer to the message at `prompt`, on one line. Empty while it has written none.
fn reply(items: &[Item], prompt: usize) -> String {
    let answer = items[prompt + 1..].iter().take_while(|item| !matches!(item, Item::User(_) | Item::UserImages(..))).find_map(|item| match item {
        Item::Text(text) if !text.trim().is_empty() => Some(text),
        _ => None,
    });
    let Some(text) = answer else {
        return String::new();
    };
    // Only the start is read: a long answer isn't walked whole for a card that clamps at three lines.
    let start: String = text.chars().take(REPLY * 2).collect();
    truncate(&start.split_whitespace().collect::<Vec<_>>().join(" "), REPLY)
}

/// The rail, over the conversation against the left of its column, which the app shows where the tab has room for it.
/// None with fewer than two prompts to move between: one tick alone at the top-left jumps nowhere.
pub fn marks(c: &Chat, rail: &Rail) -> Vec<Node> {
    if rail.prompts.len() < 2 {
        return Vec::new();
    }
    // Past `MARKS` prompts only the newest are sent, and the numbers Wings says count from the first of them.
    let skip = rail.prompts.len().saturating_sub(MARKS);
    let items: Vec<usize> = rail.prompts[skip..].iter().map(|prompt| prompt.item).collect();
    let node = Node::new("PromptRail")
        .prop("beside", COLUMN)
        .prop("marks", Value::Array(mark_values(&c.session.items, &items)))
        .prop("current", rail.current.saturating_sub(skip))
        .on("onSelect", move |payload| {
            if let Some(item) = payload["index"].as_u64().and_then(|at| items.get(at as usize)) {
                go(*item);
            }
        });
    vec![node]
}

/// What the node is sent for each prompt in `prompts` (their items' indexes): its first line and the start of the reply.
fn mark_values(items: &[Item], prompts: &[usize]) -> Vec<Value> {
    prompts.iter().map(|item| json!({ "title": first_line(&items[*item]), "preview": reply(items, *item) })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Item {
        Item::User(text.into())
    }

    /// A conversation of three messages, each answered, as the page lays it out: the rail's children are the blocks in order.
    fn talk() -> Vec<Item> {
        vec![said("Fix the build\nit fails on CI"), Item::Text("Looking".into()), said("Now the tests"), Item::Text("Done".into()), said("\n  Ship it  ")]
    }

    #[test]
    fn each_message_you_sent_is_a_prompt_and_nothing_else_is() {
        assert_eq!(prompt_items(&talk()), [0, 2, 4]);
        assert_eq!(prompt_items(&[Item::Notice("n".into()), Item::Stopped, Item::UserImages("see".into(), Vec::new())]), [2]);
        assert!(prompt_items(&[]).is_empty());
    }

    #[test]
    fn the_newest_prompt_is_in_view_until_wings_says_otherwise() {
        let built = [(0, 0), (2, 2), (4, 4)];
        let rail = Rail::of(&talk(), &built, None);
        assert_eq!((rail.current, rail.track()), (2, vec![0, 2, 4]));
        // Wings says the last tracked child at or above the top of the view is the second; or that the view is above all.
        assert_eq!(Rail::of(&talk(), &built, Some(Some(2))).current, 1);
        assert_eq!(Rail::of(&talk(), &built, Some(None)).current, 0);
        assert_eq!(Rail::of(&talk(), &built, Some(Some(9))).current, 2, "an item past the last prompt is the last");
    }

    #[test]
    fn a_resumed_session_shows_only_the_newest_prompts_and_counts_the_tracked_ones_from_the_first_drawn() {
        // Of five prompts only the newest two are drawn, as a session read back from its transcript shows its end first.
        let items: Vec<Item> = (0..5).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
        let built = [(6, 0), (8, 2)];
        let rail = Rail::of(&items, &built, Some(Some(8)));
        assert_eq!(rail.prompts.iter().map(|prompt| prompt.child).collect::<Vec<_>>(), [None, None, None, Some(0), Some(2)]);
        assert_eq!(rail.current, 4);
        assert_eq!(Rail::of(&items, &built, Some(None)).current, 3);
        assert_eq!(rail.track(), [0, 2]);
    }

    #[test]
    fn the_prompt_being_read_stays_marked_when_older_ones_load_above_it() {
        let items: Vec<Item> = (0..5).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
        // Wings said the view reached the prompt at item 8, the second of the two drawn; then two older ones load above.
        let before = Rail::of(&items, &[(6, 0), (8, 2)], Some(Some(8)));
        let after = Rail::of(&items, &[(2, 0), (4, 2), (6, 4), (8, 6)], Some(Some(8)));
        assert_eq!((before.current, after.current), (4, 4));
    }

    fn prompts_said(count: usize) -> Vec<Item> {
        (0..count).map(|n| said(&format!("Prompt {n}\nmore"))).collect()
    }

    #[test]
    fn the_rail_is_one_node_for_ten_prompts_or_for_a_thousand() {
        let build = |prompts: usize| {
            let built: Vec<(usize, usize)> = (0..prompts).map(|n| (n, n)).collect();
            chat::with(|c| c.session.items = prompts_said(prompts));
            let rail = Rail::of(&prompts_said(prompts), &built, None);
            chat::with(|c| marks(c, &rail).iter().map(Node::count).sum::<usize>())
        };
        assert_eq!((build(10), build(1_000)), (1, 1));
    }

    #[test]
    fn a_rail_of_one_prompt_draws_nothing() {
        let build = |prompts: usize| {
            let built: Vec<(usize, usize)> = (0..prompts).map(|n| (n, n)).collect();
            chat::with(|c| c.session.items = prompts_said(prompts));
            let rail = Rail::of(&prompts_said(prompts), &built, None);
            chat::with(|c| marks(c, &rail).len())
        };
        assert_eq!((build(0), build(1), build(2)), (0, 0, 1));
    }

    #[test]
    fn a_card_is_the_prompts_first_line_over_the_start_of_the_reply() {
        assert_eq!(first_line(&said("Fix the build\nit fails on CI")), "Fix the build");
        assert_eq!(first_line(&said("\n  Ship it  ")), "Ship it");
        assert_eq!(first_line(&Item::UserImages(String::new(), Vec::new())), "Picture");
        assert!(first_line(&said(&"x".repeat(500))).chars().count() <= LINE + 1);
        let items = vec![
            said("Fix it"),
            Item::Worked(5),
            Item::Text("  \n".into()),
            Item::Text("I found\n\n  the bug in `build.rs`.".into()),
            said("And now?"),
            Item::Stopped,
            said("Again"),
            Item::Text("x".repeat(2_000)),
        ];
        assert_eq!(reply(&items, 0), "I found the bug in `build.rs`.");
        assert_eq!(reply(&items, 4), "", "a prompt Claude hasn't answered has no reply, not the next prompt's");
        assert!(reply(&items, 6).chars().count() <= REPLY + 1);
        let values = mark_values(&items, &[0, 4]);
        assert_eq!(values[0], json!({ "title": "Fix it", "preview": "I found the bug in `build.rs`." }));
        assert_eq!(values[1], json!({ "title": "And now?", "preview": "" }));
    }

    #[test]
    fn a_click_scrolls_the_conversation_to_that_prompts_node_once() {
        let items = talk();
        let built = [(0, 0), (2, 2), (4, 4)];
        chat::with(|c| c.session.items = talk());
        // Nothing is scrolled to before a click.
        assert_eq!(scroll_to(&Rail::of(&items, &built, None), 0), None);
        go(2);
        let to = scroll_to(&Rail::of(&items, &built, None), 0).expect("the second prompt's place");
        assert_eq!(to, json!({ "index": 2, "token": 1 }));
        // The same place keeps being said, with the same token, which Wings acts on once; a click on another has the next.
        assert_eq!(scroll_to(&Rail::of(&items, &built, None), 0), Some(to));
        go(4);
        assert_eq!(scroll_to(&Rail::of(&items, &built, None), 0), Some(json!({ "index": 4, "token": 2 })));
        // The column has grown a block above since: the same prompt, a new index, the same token.
        assert_eq!(scroll_to(&Rail::of(&items, &[(0, 1), (2, 3), (4, 5)], None), 0), Some(json!({ "index": 5, "token": 2 })));
    }

    #[test]
    fn a_click_on_a_prompt_older_than_the_page_shows_brings_it_in_and_one_it_cant_draw_is_given_up() {
        let items: Vec<Item> = (0..300).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
        chat::with(|c| {
            c.session.items = (0..300).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
            c.shown = crate::chat::PAGE;
        });
        go(100);
        let reached = chat::with(|c| {
            let end = crate::view::end_of(c);
            crate::view::window(c, end, end.saturating_sub(c.shown)).0.last().is_some_and(|block| block.start <= 100 && 100 < end)
        });
        assert!(reached, "the page now shows it");
        // The page drew from the newest back and ran out of room before it: no place, and no jump left pending.
        let built: Vec<(usize, usize)> = (500..600).step_by(2).enumerate().map(|(child, item)| (item, child)).collect();
        assert_eq!(scroll_to(&Rail::of(&items, &built, None), 0), None);
        let all: Vec<(usize, usize)> = (0..300).map(|n| (n * 2, n)).collect();
        assert_eq!(scroll_to(&Rail::of(&items, &all, None), 0), None, "it stays given up when scrolling up draws it");
    }
}
