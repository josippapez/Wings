//! Bookmarks on messages: a button in the row under your message and under Claude's reply, and a Bookmarks section in the
//! session panel (`view::side_sections`) whose rows scroll the conversation to the message (as a click on the prompt rail
//! does) and remove the bookmark.
//!
//! They are kept in the plugin's storage under `bookmarks/<session id>`, so they come back when you resume the session.
//! A message has no id in the session, so a bookmark keeps its place among the items and the start of the message: when
//! the item at that place no longer starts so (after a rewind, or a compacted session read back), the first message that
//! does is the bookmark's. One that finds none stays stored and hidden, since the session may still be loading.

use serde_json::{Value, json};
use wings_plugin::{
    host::storage,
    ui::{Entry, Node, button, entry, icon, muted, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    session::Item,
};

/// How many characters of a message identify it, and show in the panel.
const START: usize = 80;

const BOOKMARK: [&str; 1] = ["m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"];
const REMOVE: [&str; 2] = ["M18 6 6 18", "m6 6 12 12"];

#[derive(Clone, Debug, PartialEq)]
struct Mark {
    /// The item it was last seen at.
    at: usize,
    /// The start of its message.
    text: String,
    /// Whether the conversation has that message now.
    found: bool,
}

#[derive(Default)]
pub struct Bookmarks {
    /// The session the marks were read for.
    session: Option<String>,
    marks: Vec<Mark>,
    /// How many items the conversation had when the marks were last placed, and none until they have been.
    placed: Option<usize>,
}

/// What a message starts with, for the messages you can bookmark: yours and Claude's replies.
fn start_of(item: &Item) -> Option<String> {
    let (Item::User(said) | Item::UserImages(said, _) | Item::Text(said)) = item else {
        return None;
    };
    let start: String = said.trim().chars().take(START).collect();
    (!start.is_empty()).then_some(start)
}

/// Whether `item` is the message that starts with `text`, without making its start to compare.
fn starts(item: &Item, text: &str) -> bool {
    let (Item::User(said) | Item::UserImages(said, _) | Item::Text(said)) = item else {
        return false;
    };
    said.trim().chars().take(START).eq(text.chars())
}

impl Bookmarks {
    pub fn has(&self, at: usize) -> bool {
        self.marks.iter().any(|mark| mark.found && mark.at == at)
    }

    /// The bookmarks the conversation has, oldest first.
    fn listed(&self) -> Vec<&Mark> {
        let mut marks: Vec<&Mark> = self.marks.iter().filter(|mark| mark.found).collect();
        marks.sort_by_key(|mark| mark.at);
        marks
    }

    fn read(stored: &Value) -> Vec<Mark> {
        let marks = stored.as_array().into_iter().flatten();
        marks.filter_map(|mark| Some(Mark { at: mark["at"].as_u64()? as usize, text: mark["text"].as_str()?.to_owned(), found: false })).collect()
    }

    fn json(&self) -> Value {
        Value::Array(self.marks.iter().map(|mark| json!({ "at": mark.at, "text": mark.text })).collect())
    }

    /// Finds each mark's message: where it was, or else the first message that starts as it did.
    fn place(&mut self, items: &[Item]) {
        for mark in &mut self.marks {
            let there = items.get(mark.at).is_some_and(|item| starts(item, &mark.text));
            match there.then_some(mark.at).or_else(|| items.iter().position(|item| starts(item, &mark.text))) {
                Some(at) => (mark.at, mark.found) = (at, true),
                None => mark.found = false,
            }
        }
        self.placed = Some(items.len());
    }

    /// Adds the bookmark on the message at `at`, or takes it off. False for an item that can't be bookmarked.
    fn toggle(&mut self, items: &[Item], at: usize) -> bool {
        if let Some(mark) = self.marks.iter().position(|mark| mark.found && mark.at == at) {
            self.marks.remove(mark);
            return true;
        }
        let Some(text) = items.get(at).and_then(start_of) else {
            return false;
        };
        self.marks.push(Mark { at, text, found: true });
        true
    }
}

fn key_of(session: &str) -> String {
    let id: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).take(100).collect();
    format!("bookmarks/{id}")
}

/// Reads this session's bookmarks when the chat has come to know its session, and places them as the conversation grows.
/// Called as the page builds, and cheap while nothing changed: two comparisons.
pub fn refresh(c: &mut Chat) {
    let (items, id, marks) = (&c.session.items, &c.session.id, &mut c.bookmarks);
    if marks.session != *id {
        marks.session = id.clone();
        marks.placed = None;
        marks.marks = id.as_deref().and_then(|id| storage::get(&key_of(id)).ok()).map(|stored| Bookmarks::read(&stored)).unwrap_or_default();
    }
    if !marks.marks.is_empty() && marks.placed != Some(items.len()) {
        marks.place(items);
    }
}

fn save(c: &Chat) {
    if let Some(id) = &c.session.id {
        drop(storage::set(&key_of(id), &c.bookmarks.json()));
    }
}

fn toggle(at: usize) {
    chat::with(|c| {
        let items = &c.session.items;
        if c.bookmarks.toggle(items, at) {
            save(c);
        }
    });
}

/// The button that bookmarks the message at `at`, lit while it is. None for a chat with no session yet, which has nowhere to
/// keep it, and for a message with no words.
pub fn toggle_button(at: usize, c: &Chat) -> Option<Node> {
    let kept = c.bookmarks.has(at);
    if !kept && (c.session.id.is_none() || c.session.items.get(at).and_then(start_of).is_none()) {
        return None;
    }
    let label = if kept { "Remove the bookmark" } else { "Bookmark this message" };
    let button = button("", if kept { "secondary" } else { "ghost" }, false, move || toggle(at));
    Some(button.prop("size", "icon-sm").prop("label", label).prop("title", label).child(icon(24, 2.0, &BOOKMARK).prop("size", 14)).key(format!("bookmark-{at}")))
}


/// `other` (the buttons beside a message already) with the bookmark button after them.
pub fn with_button(at: usize, c: &Chat, other: Option<Node>) -> Option<Node> {
    match (other, toggle_button(at, c)) {
        (None, None) => None,
        (other, mark) => Some(row().prop("gap", "xs").prop("align", "center").maybe(other).maybe(mark)),
    }
}

/// The panel's section: each bookmarked message's start, which scrolls to it, and a button that removes it.
pub fn section(c: &Chat) -> Option<Node> {
    let marks = c.bookmarks.listed();
    if marks.is_empty() {
        return None;
    }
    let rows = marks.iter().map(|mark| row_of(mark, &c.session.items));
    Some(crate::view::fold_list(c, "bookmarks", format!("Bookmarks · {}", marks.len()), rows))
}

fn row_of(mark: &Mark, items: &[Item]) -> Node {
    let at = mark.at;
    let who = if matches!(items.get(at), Some(Item::Text(_))) { "Claude" } else { "You" };
    // Markdown's backticks are left out: the line is plain text and they would show as they are.
    let line = text(mark.text.lines().next().unwrap_or_default().replace('`', "")).prop("size", "small").prop("clamp", true).prop("title", mark.text.as_str());
    let jump = entry(Entry { lead: None, title: line, detail: Some(muted(who).prop("size", "small")), trailing: None }).on("onClick", move |_| crate::rail::go(at));
    let remove = button("", "ghost", false, move || toggle(at)).prop("size", "icon-sm").prop("label", "Remove the bookmark").prop("title", "Remove this bookmark").child(icon(24, 2.0, &REMOVE).prop("size", 14));
    stack().prop("gap", "none").key(format!("bookmark-{at}")).child(row().prop("gap", "xs").prop("align", "center").child(stack().prop("grow", true).child(jump)).child(remove))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn talk() -> Vec<Item> {
        vec![Item::User("Fix the build".into()), Item::Text("Looking at it.\nMore.".into()), Item::Stopped, Item::User("Now the tests".into()), Item::Text("Done".into())]
    }

    #[test]
    fn only_a_message_with_words_can_be_bookmarked_and_the_same_click_takes_it_off() {
        let items = talk();
        let mut marks = Bookmarks::default();
        assert!(!marks.toggle(&items, 2), "a stop is no message");
        assert!(marks.toggle(&items, 1));
        assert!(marks.has(1) && !marks.has(0));
        assert_eq!(marks.listed().iter().map(|mark| mark.text.as_str()).collect::<Vec<_>>(), ["Looking at it.\nMore."]);
        assert!(marks.toggle(&items, 1));
        assert!(marks.listed().is_empty());
        assert!(!marks.toggle(&[Item::UserImages("  ".into(), vec!["a".into()])], 0), "words only");
    }

    #[test]
    fn a_bookmark_comes_back_from_storage_where_its_message_is_even_after_the_items_moved() {
        let mut marks = Bookmarks::default();
        assert!(marks.toggle(&talk(), 3));
        assert!(marks.toggle(&talk(), 4));
        // What was stored reads back as it was, not yet found in any conversation.
        let mut back = Bookmarks { marks: Bookmarks::read(&marks.json()), ..Default::default() };
        assert_eq!(back.marks.len(), 2);
        assert!(back.listed().is_empty());
        // The session comes back as it was.
        back.place(&talk());
        assert_eq!(back.listed().iter().map(|mark| mark.at).collect::<Vec<_>>(), [3, 4]);
        // Two items went (a rewind, a compaction): the messages are found by their words, at their new places.
        let moved: Vec<Item> = talk().into_iter().skip(2).collect();
        back.place(&moved);
        assert_eq!(back.listed().iter().map(|mark| mark.at).collect::<Vec<_>>(), [1, 2]);
        // A message that is gone stays stored, hidden, and shows again when it is back.
        back.place(&talk()[..3]);
        assert!(back.listed().is_empty());
        assert_eq!(back.marks.len(), 2);
        back.place(&talk());
        assert_eq!(back.listed().len(), 2);
        // Stored junk is skipped.
        assert!(Bookmarks::read(&json!([{ "at": "x" }, 4, { "at": 1, "text": "ok" }])).iter().all(|mark| mark.text == "ok"));
        assert!(Bookmarks::read(&json!("nope")).is_empty());
    }

    #[test]
    fn a_session_is_kept_under_its_own_key_made_of_what_storage_allows() {
        assert_eq!(key_of("0c3e-9f"), "bookmarks/0c3e-9f");
        assert_eq!(key_of("../a b/c"), "bookmarks/abc");
        assert!(key_of(&"a".repeat(500)).len() <= 128);
    }

    #[test]
    fn the_panel_lists_bookmarks_oldest_first_and_the_buttons_show_which_are_lit() {
        chat::with(|c| {
            c.session.id = Some("s1".into());
            c.session.items = talk();
        });
        assert!(chat::with(|c| section(c).is_none()));
        toggle(4);
        toggle(0);
        let (rows, lit, unlit) = chat::with(|c| (c.bookmarks.listed().iter().map(|mark| mark.at).collect::<Vec<_>>(), toggle_button(4, c).is_some(), toggle_button(2, c).is_none()));
        assert_eq!(rows, [0, 4]);
        assert!(lit && unlit);
        assert!(chat::with(|c| section(c).is_some()));
        // No session, nowhere to keep one: no button.
        chat::with(|c| c.session.id = None);
        assert!(chat::with(|c| toggle_button(1, c).is_none() && with_button(1, c, None).is_none()));
    }

    #[test]
    fn a_bookmarks_line_shows_no_markdown_backticks() {
        let mark = Mark { at: 1, text: "Now in `src/discount.ts` and `checkout.ts`".into(), found: true };
        let shown = wings_plugin::ui::tree_json(row_of(&mark, &talk())).to_string();
        assert!(shown.contains("\"text\":\"Now in src/discount.ts and checkout.ts\""), "{shown}");
    }
}
