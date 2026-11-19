//! The rail at the left of the conversation: one small mark for each message you sent (queued ones aren't in the
//! conversation yet, so they have none), the mark of the prompt you are reading lit, a tooltip with the prompt's first
//! line, and a click that scrolls the conversation so that prompt is at its top.
//!
//! Where the marks sit: evenly, from the top. The page can't know how tall what it sends will be drawn, so it can't space
//! them by where each prompt is in the conversation. A session with more prompts than `MARKS` groups them, each mark
//! standing for the run it starts, so the rail stays a few dozen nodes however long the session is.
//!
//! How it scrolls and knows where you are: the conversation's `Scroll` takes `scrollTo` (the child of its column to bring
//! to the top, once for each token) and `track` with `onTrack` (which prompt's child the view has reached), both Wings
//! 0.2.42. The page tells Wings only the children's numbers; Wings reads their tops while it draws and tells the page
//! only when the prompt in view changes, so reading a long answer costs the page nothing.

use std::cell::RefCell;

use serde_json::{Value, json};
use wings_plugin::{
    format::truncate,
    ui::{Node, button, dot, stack},
};

use crate::{
    chat::{self, Chat},
    session::Item,
    view::COLUMN,
};

/// How wide the rail is, in points.
pub const WIDTH: u32 = 28;
/// The tab width below which the rail is left out: the column with its margins, and the rail either side of it.
pub const BELOW: u32 = COLUMN + 16 + 2 * WIDTH;
/// The most marks the rail draws.
pub const MARKS: usize = 24;
/// The longest first line a tooltip shows.
const LINE: usize = 90;

#[derive(Default)]
struct State {
    /// The prompt (as its item's index) a click asked for, until it is scrolled to or can't be.
    jump: Option<usize>,
    /// Each click has the next one, so Wings scrolls once for each.
    token: u64,
    /// What Wings last said: none before it has said, then the position in the tracked prompts, or none above them all.
    at: Option<Option<usize>>,
    /// Whether the tab is narrower than `BELOW`, as the rail's split says.
    narrow: Option<bool>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Whether the rail has room, as the tab's width says: no, until the split has measured it.
pub fn roomy() -> bool {
    STATE.with(|state| state.borrow().narrow == Some(false))
}

pub fn set_narrow(narrow: bool) {
    STATE.with(|state| state.borrow_mut().narrow = Some(narrow));
}

/// What Wings says of the tracked prompts: the position of the last whose top the view has reached.
pub fn tracked(at: Option<usize>) {
    STATE.with(|state| state.borrow_mut().at = Some(at));
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
    pub fn of(items: &[Item], built: &[(usize, usize)], at: Option<Option<usize>>) -> Rail {
        let prompts: Vec<Prompt> = prompt_items(items)
            .into_iter()
            .map(|item| Prompt { item, child: built.binary_search_by_key(&item, |(built, _)| *built).ok().map(|at| built[at].1) })
            .collect();
        // Those drawn are the newest, so the tracked positions count from the first of them.
        let first = prompts.iter().position(|prompt| prompt.child.is_some());
        let current = match (at, first) {
            (Some(Some(at)), Some(first)) => (first + at).min(prompts.len() - 1),
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

/// The mark whose run holds the prompt `current`, among `total`.
pub fn lit_mark(total: usize, current: usize) -> Option<usize> {
    groups(total).iter().position(|(start, end)| (*start..*end).contains(&current))
}

/// The runs of prompts the marks stand for, as (start, end) over `total` prompts, at most `MARKS`.
pub fn groups(total: usize) -> Vec<(usize, usize)> {
    let marks = total.min(MARKS);
    (0..marks).map(|mark| (mark * total / marks, (mark + 1) * total / marks)).collect()
}

/// A click on a mark: the conversation scrolls so `item`, a message of yours, is at its top. A prompt older than what the
/// page shows is brought in first.
pub fn go(item: usize) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.token += 1;
        state.jump = Some(item);
    });
    chat::with(|c| c.shown = c.shown.max(c.session.items.len().saturating_sub(item)));
}

/// What the conversation's `Scroll` is told after a click: the child to bring to the top, with the click's token. The page
/// keeps saying it, which Wings acts on once. A prompt the page couldn't draw (it is past what the page may send) is given
/// up on, so it isn't scrolled to later, when scrolling up draws it.
pub fn scroll_to(rail: &Rail) -> Option<Value> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
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

/// What the page says of the rail to the conversation's `Scroll`: its props, and `None` while there are no prompts.
pub fn watch(scroll: Node, rail: &Rail) -> Node {
    if rail.prompts.is_empty() {
        return scroll;
    }
    let scroll = scroll.prop("track", json!(rail.track())).on("onTrack", |payload| tracked(payload["at"].as_u64().map(|at| at as usize)));
    match scroll_to(rail) {
        Some(to) => scroll.prop("scrollTo", to),
        None => scroll,
    }
}

/// The first line of a message, as the tooltip says it.
fn first_line(item: &Item) -> String {
    let (Item::User(said) | Item::UserImages(said, _)) = item else {
        return String::new();
    };
    match said.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => truncate(line, LINE),
        None => "Picture".to_owned(),
    }
}

/// The rail's marks, for the slot at the left of the conversation. None while there is no room, nothing to mark, or the
/// tab hasn't been measured.
pub fn marks(c: &Chat, rail: &Rail) -> Vec<Node> {
    if !roomy() || rail.prompts.is_empty() {
        return Vec::new();
    }
    let lit_at = lit_mark(rail.prompts.len(), rail.current);
    let marks = groups(rail.prompts.len()).into_iter().enumerate().map(|(at, (start, end))| {
        let lit = lit_at == Some(at);
        let item = rail.prompts[start].item;
        let line = first_line(&c.session.items[item]);
        let said = if end - start > 1 { format!("{line} (and {} after it)", end - start - 1) } else { line };
        let tone = if lit { "info" } else { "muted" };
        button("", if lit { "secondary" } else { "ghost" }, false, move || go(item))
            .prop("size", "icon-xs")
            .prop("label", said.as_str())
            .prop("title", said.as_str())
            .child(dot().prop("tone", tone))
            .key(item.to_string())
    });
    vec![stack().prop("gap", "xs").children(marks)]
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
        assert_eq!(Rail::of(&talk(), &built, Some(Some(1))).current, 1);
        assert_eq!(Rail::of(&talk(), &built, Some(None)).current, 0);
        assert_eq!(Rail::of(&talk(), &built, Some(Some(9))).current, 2, "a position past the last is the last");
    }

    #[test]
    fn a_resumed_session_shows_only_the_newest_prompts_and_counts_the_tracked_ones_from_the_first_drawn() {
        // Of five prompts only the newest two are drawn, as a session read back from its transcript shows its end first.
        let items: Vec<Item> = (0..5).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
        let built = [(6, 0), (8, 2)];
        let rail = Rail::of(&items, &built, Some(Some(1)));
        assert_eq!(rail.prompts.iter().map(|prompt| prompt.child).collect::<Vec<_>>(), [None, None, None, Some(0), Some(2)]);
        assert_eq!(rail.current, 4);
        assert_eq!(Rail::of(&items, &built, Some(None)).current, 3);
        assert_eq!(rail.track(), [0, 2]);
    }

    #[test]
    fn a_long_session_has_a_few_dozen_marks_that_cover_every_prompt_once() {
        assert_eq!(groups(0), []);
        assert_eq!(groups(3), [(0, 1), (1, 2), (2, 3)]);
        for total in [24, 25, 100, 999] {
            let runs = groups(total);
            assert_eq!(runs.len(), MARKS);
            assert_eq!(runs[0].0, 0);
            assert_eq!(runs.last().unwrap().1, total);
            assert!(runs.windows(2).all(|pair| pair[0].1 == pair[1].0 && pair[0].0 < pair[0].1), "{total}");
        }
    }

    fn prompts_said(count: usize) -> Vec<Item> {
        (0..count).map(|n| said(&format!("Prompt {n}\nmore"))).collect()
    }

    #[test]
    fn the_rail_costs_the_same_few_nodes_for_ten_prompts_or_five_hundred() {
        set_narrow(false);
        let build = |prompts: usize| {
            let built: Vec<(usize, usize)> = (0..prompts).map(|n| (n, n)).collect();
            chat::with(|c| c.session.items = prompts_said(prompts));
            let rail = Rail::of(&prompts_said(prompts), &built, None);
            chat::with(|c| marks(c, &rail).iter().map(Node::count).sum::<usize>())
        };
        let (few, many) = (build(10), build(500));
        assert!(few > 0 && many <= MARKS * 4 + 2, "{few} {many}");
        assert!(many < 3 * few + 60, "{few} {many}");
    }

    #[test]
    fn the_lit_mark_is_the_one_whose_run_holds_the_prompt_in_view() {
        assert_eq!(lit_mark(8, 3), Some(3));
        assert_eq!(lit_mark(8, 7), Some(7));
        // Fifty prompts in 24 marks: the run that holds the prompt.
        let runs = groups(50);
        let lit = lit_mark(50, 30).unwrap();
        assert!((runs[lit].0..runs[lit].1).contains(&30));
        assert_eq!(lit_mark(0, 0), None);
        // The marks are as many as the groups, and one of them is built lit.
        set_narrow(false);
        chat::with(|c| c.session.items = prompts_said(8));
        let built: Vec<(usize, usize)> = (0..8).map(|n| (n, n)).collect();
        let (dim, lit) = (Rail::of(&prompts_said(8), &built, Some(Some(0))), Rail::of(&prompts_said(8), &built, Some(Some(5))));
        assert_eq!((dim.current, lit.current), (0, 5));
        assert_eq!(chat::with(|c| marks(c, &lit)[0].count()), chat::with(|c| marks(c, &dim)[0].count()));
    }

    #[test]
    fn a_tooltip_is_the_prompts_first_line_and_says_how_many_a_mark_stands_for() {
        assert_eq!(first_line(&said("Fix the build\nit fails on CI")), "Fix the build");
        assert_eq!(first_line(&said("\n  Ship it  ")), "Ship it");
        assert_eq!(first_line(&Item::UserImages(String::new(), Vec::new())), "Picture");
        assert!(first_line(&said(&"x".repeat(500))).chars().count() <= LINE + 1);
    }

    #[test]
    fn a_click_scrolls_the_conversation_to_that_prompts_node_once() {
        let items = talk();
        let built = [(0, 0), (2, 2), (4, 4)];
        chat::with(|c| c.session.items = talk());
        // Nothing is scrolled to before a click.
        assert_eq!(scroll_to(&Rail::of(&items, &built, None)), None);
        go(2);
        let to = scroll_to(&Rail::of(&items, &built, None)).expect("the second prompt's place");
        assert_eq!(to, json!({ "index": 2, "token": 1 }));
        // The same place keeps being said, with the same token, which Wings acts on once; a click on another has the next.
        assert_eq!(scroll_to(&Rail::of(&items, &built, None)), Some(to));
        go(4);
        assert_eq!(scroll_to(&Rail::of(&items, &built, None)), Some(json!({ "index": 4, "token": 2 })));
        // The column has grown a block above since: the same prompt, a new index, the same token.
        assert_eq!(scroll_to(&Rail::of(&items, &[(0, 1), (2, 3), (4, 5)], None)), Some(json!({ "index": 5, "token": 2 })));
    }

    #[test]
    fn a_click_on_a_prompt_older_than_the_page_shows_brings_it_in_and_one_it_cant_draw_is_given_up() {
        let items: Vec<Item> = (0..300).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
        chat::with(|c| {
            c.session.items = (0..300).flat_map(|n| [said(&format!("Prompt {n}")), Item::Text("ok".into())]).collect();
            c.shown = crate::chat::PAGE;
        });
        go(100);
        assert!(chat::with(|c| c.session.items.len() - c.shown <= 100), "the page now reaches it");
        // The page drew from the newest back and ran out of room before it: no place, and no jump left pending.
        let built: Vec<(usize, usize)> = (500..600).step_by(2).enumerate().map(|(child, item)| (item, child)).collect();
        assert_eq!(scroll_to(&Rail::of(&items, &built, None)), None);
        let all: Vec<(usize, usize)> = (0..300).map(|n| (n * 2, n)).collect();
        assert_eq!(scroll_to(&Rail::of(&items, &all, None)), None, "it stays given up when scrolling up draws it");
    }

    #[test]
    fn the_rail_is_left_out_of_a_tab_too_narrow_to_hold_it_beside_the_column() {
        assert!(!roomy(), "not before the tab is measured");
        set_narrow(true);
        assert!(!roomy());
        set_narrow(false);
        assert!(roomy());
        // The column with its margins and the rail at each side fit in the width the rail needs.
        assert!(BELOW >= COLUMN + 16 + 2 * WIDTH);
    }
}
