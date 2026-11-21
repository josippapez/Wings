//! Search in a chat: a bar under the header (the header's button, or Cmd-Shift-F) with a field, "3 of 12", and previous and
//! next buttons (Enter and Shift-Enter in the field, Cmd-G and Cmd-Shift-G anywhere in the tab). Escape closes it. Cmd-F is
//! Wings' own for terminals, which a page's shortcut can't take (`Shortcut::parse` in the app refuses it).
//!
//! It looks through the text of your messages and Claude's, what Claude thought, errors and notes, and each tool call's name,
//! one-line summary and input (a command, the lines an edit changes, the start of a file written). What a tool printed is
//! left out: it is long, and Verbose (`view_mode.rs`) is for reading it. A hit is one item, so "12" counts items.
//!
//! Cost: the conversation is read when the query changes and then only for items that arrive after, never once for each
//! frame (`refresh`), and nothing is read while the bar is closed. A jump brings the hit into what the page shows
//! (`view::show_item`), opens the run of tool calls it is in, and scrolls to it as a click on the prompt rail does.

use wings_plugin::ui::{Node, badge, button, icon, muted, row, stack, text_input};

use crate::{
    chat::{self, Chat},
    folds::Kind,
    session::{Ask, Detail, Item},
    view::{folds, icon_button},
};

const SEARCH: [&str; 2] = ["M3 11a8 8 0 1 0 16 0a8 8 0 1 0-16 0", "m21 21-4.3-4.3"];
const UP: [&str; 1] = ["m18 15-6-6-6 6"];
const DOWN: [&str; 1] = ["m6 9 6 6 6-6"];
const CLOSE: [&str; 2] = ["M18 6 6 18", "m6 6 12 12"];

#[derive(Default)]
pub struct Search {
    pub open: bool,
    /// What the field holds.
    pub query: String,
    /// The query as it is looked for: trimmed and lower case. Empty finds nothing.
    needle: String,
    /// The items that match, oldest first.
    hits: Vec<usize>,
    /// How many items of the conversation `hits` has looked through.
    scanned: usize,
    /// The hit in use, as its place in `hits`.
    at: usize,
}

impl Search {
    /// The item of the hit in use, while the bar is open.
    pub fn current(&self) -> Option<usize> {
        self.open.then(|| self.hits.get(self.at).copied()).flatten()
    }

    fn rescan(&mut self) {
        self.hits.clear();
        self.scanned = 0;
    }

    /// "3 of 12", "No matches", or nothing while the field is empty.
    fn count(&self) -> Option<String> {
        match (self.needle.is_empty(), self.hits.len()) {
            (true, _) => None,
            (false, 0) => Some("No matches".to_owned()),
            (false, n) => Some(format!("{} of {n}", self.at + 1)),
        }
    }
}

/// Whether `hay` holds `needle` (lower case, not empty) in any case.
fn contains(hay: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        let needle = needle.as_bytes();
        hay.as_bytes().windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle))
    } else {
        hay.to_lowercase().contains(needle)
    }
}

/// Whether any of what an item says, or what a tool call was asked, holds `needle`.
fn matches(item: &Item, needle: &str) -> bool {
    let has = |said: &str| contains(said, needle);
    match item {
        Item::User(said) | Item::UserImages(said, _) | Item::Text(said) | Item::Error(said) | Item::Notice(said) | Item::Divider(said) | Item::Thought(said, _) => has(said),
        Item::Note(title, body) => has(title) || has(body),
        Item::Finished(done) => has(&done.description) || has(&done.report),
        Item::Ask(Ask { tool, summary, detail, .. }) => has(tool) || has(summary) || has(detail),
        Item::Tool(tool) => {
            has(&tool.name)
                || has(&tool.summary)
                || match &tool.detail {
                    Detail::Command(command) => has(command),
                    Detail::Diff(diff) => diff.lines.iter().any(|(_, line)| has(line)),
                    Detail::Write { head, .. } => head.iter().any(|line| has(line)),
                    Detail::Plain | Detail::Read(_) | Detail::Search => false,
                }
        }
        _ => false,
    }
}

/// Reads the items that came since the last read, while the bar is open with something in the field. Called as the page builds.
pub fn refresh(c: &mut Chat) {
    let (items, search) = (&c.session.items, &mut c.search);
    if !search.open || search.needle.is_empty() {
        return;
    }
    // A rewind took items away: what was found among them goes with them.
    if items.len() < search.scanned {
        search.rescan();
    }
    let needle = &search.needle;
    search.hits.extend(items.iter().enumerate().skip(search.scanned).filter(|(_, item)| matches(item, needle)).map(|(at, _)| at));
    search.scanned = items.len();
    search.at = search.at.min(search.hits.len().saturating_sub(1));
}

/// The field changed: it looks for the new words from the start and goes to the newest hit, where a reader of a chat is.
fn set_query(query: String) {
    chat::with(|c| {
        c.search.needle = query.trim().to_lowercase();
        c.search.query = query;
        c.search.rescan();
        refresh(c);
        c.search.at = c.search.hits.len().saturating_sub(1);
        reveal(c);
    });
}

/// Goes to the hit in use: its run of tool calls opens, and the conversation scrolls to it.
fn reveal(c: &mut Chat) {
    let Some(item) = c.search.current() else {
        return;
    };
    if folds(&c.session.items[item]) {
        let mut first = item;
        while first > 0 && folds(&c.session.items[first - 1]) {
            first -= 1;
        }
        // A click on a run flips what its mode shows, so a shut run is opened by flipping it.
        if !crate::folds::is_open(c, Kind::Steps, c.open_steps.contains(&first)) && !c.open_steps.remove(&first) {
            c.open_steps.insert(first);
        }
    }
    crate::rail::go_in(c, item);
}

fn step(forward: bool) {
    chat::with(|c| {
        let n = c.search.hits.len();
        if n == 0 {
            return;
        }
        c.search.at = if forward { (c.search.at + 1) % n } else { (c.search.at + n - 1) % n };
        reveal(c);
    });
}

fn open_bar() {
    chat::with(|c| {
        c.search.open = true;
        // What came while it was closed was not read.
        c.search.rescan();
    });
}

fn close() {
    chat::with(|c| {
        c.search.open = false;
        c.search.rescan();
    });
}

/// The header's button, which opens the bar and shuts it again.
pub fn header_button(c: &Chat) -> Node {
    let (label, on) = if c.search.open { ("Close search", close as fn()) } else { ("Search", open_bar as fn()) };
    icon_button(label, &SEARCH, on).shortcut("cmd-shift-f")
}

/// The bar under the header while search is open.
pub fn bar(c: &Chat) -> Option<Node> {
    let search = &c.search;
    if !search.open {
        return None;
    }
    let field = text_input("Search the conversation", &search.query, set_query)
        .prop("placeholder", "Search messages and tool calls")
        .prop("autoFocus", true)
        .on("onKeyDown", |event| match event["key"].as_str() {
            Some("Enter") => step(event["shiftKey"] != true),
            Some("Escape") => close(),
            _ => {}
        });
    let none = search.hits.is_empty();
    let nav = |label: &str, paths: &[&str], forward: bool, keys: &str| {
        button("", "ghost", none, move || step(forward)).prop("size", "icon-sm").prop("label", label).prop("title", label).shortcut(keys).child(icon(24, 2.0, paths).prop("size", 16))
    };
    let line = row()
        .prop("gap", "m")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(field))
        .maybe(search.count().map(|said| muted(said).prop("size", "small")))
        .child(nav("Previous match", &UP, false, "cmd-shift-g"))
        .child(nav("Next match", &DOWN, true, "cmd-g"))
        .child(icon_button("Close search", &CLOSE, close));
    Some(Node::new("Bar").prop("variant", "slim").child(line).key("search"))
}

/// The block of items `start..end` as the page builds it, with "3 of 12" over it when the hit in use is in it.
pub fn mark(c: &Chat, start: usize, end: usize, node: Node) -> Node {
    // Every block asks, so the count is only made for the one that has the hit.
    match c.search.current().filter(|item| (start..end).contains(item)).and_then(|_| c.search.count()) {
        Some(said) => stack().prop("gap", "xs").child(row().child(badge(said, Some("info")))).child(node).key(format!("hit-{start}")),
        None => node,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Diff, Tool};

    fn tool(name: &str, summary: &str, detail: Detail) -> Item {
        Item::Tool(Tool { id: format!("{name}-{summary}"), name: name.into(), summary: summary.into(), output: Some(("printed NEEDLE".into(), false)), detail, input: Default::default(), images: Vec::new() })
    }

    fn talk() -> Vec<Item> {
        vec![
            Item::User("Fix the Build".into()),
            Item::Thought("the build script is odd".into(), 0),
            tool("Bash", "make", Detail::Command("make build --release".into())),
            tool("Edit", "src/lib.rs", Detail::Diff(Diff { added: 1, removed: 0, lines: vec![(true, "let Needle = 1;".into())], ..Default::default() })),
            Item::Text("Done, the build passes.".into()),
            Item::Stopped,
        ]
    }

    fn found(query: &str, items: &[Item]) -> Vec<usize> {
        let needle = query.trim().to_lowercase();
        items.iter().enumerate().filter(|(_, item)| matches(item, &needle)).map(|(at, _)| at).collect()
    }

    #[test]
    fn it_finds_text_thoughts_and_a_tool_calls_name_and_input_whatever_the_case_and_not_what_it_printed() {
        let items = talk();
        assert_eq!(found("BUILD", &items), [0, 1, 2, 4]);
        assert_eq!(found("release", &items), [2], "a command's whole input");
        assert_eq!(found("needle", &items), [3], "a changed line, and not the output of a call");
        assert_eq!(found("edit", &items), [3], "a tool's name");
        assert_eq!(found("zzz", &items), Vec::<usize>::new());
        // Words that are not ASCII are matched in any case too.
        assert!(matches(&Item::Text("Grüße aus Zürich".into()), "zürich"));
    }

    #[test]
    fn the_conversation_is_read_when_the_query_changes_and_after_that_only_for_what_arrives() {
        chat::with(|c| c.session.items = talk());
        open_bar();
        set_query("build".into());
        assert_eq!(chat::with(|c| (c.search.hits.clone(), c.search.at, c.search.scanned)), (vec![0, 1, 2, 4], 3, 6));
        // An item that arrives is read alone, and the hit in use stays.
        chat::with(|c| {
            c.search.at = 1;
            c.session.items.push(Item::Text("Build again".into()));
            refresh(c);
        });
        assert_eq!(chat::with(|c| (c.search.hits.clone(), c.search.at, c.search.scanned)), (vec![0, 1, 2, 4, 6], 1, 7));
        // With nothing new nothing is read again: a hit planted in `hits` stays where it is.
        chat::with(|c| {
            c.search.hits.push(99);
            refresh(c);
            assert_eq!(c.search.hits.last(), Some(&99));
            c.search.hits.pop();
        });
        // A rewind that takes items away is read again from the start.
        chat::with(|c| {
            c.session.items.truncate(3);
            refresh(c);
        });
        assert_eq!(chat::with(|c| c.search.hits.clone()), [0, 1, 2]);
        // The field's words are trimmed.
        set_query("  Make ".into());
        assert_eq!(chat::with(|c| c.search.hits.clone()), [2]);
    }

    #[test]
    fn next_and_previous_go_round_and_show_where_you_are() {
        chat::with(|c| c.session.items = talk());
        open_bar();
        set_query("build".into());
        assert_eq!(chat::with(|c| c.search.count()).as_deref(), Some("4 of 4"));
        step(true);
        assert_eq!(chat::with(|c| (c.search.at, c.search.current())), (0, Some(0)));
        step(false);
        assert_eq!(chat::with(|c| c.search.current()), Some(4));
        set_query("zzz".into());
        assert_eq!(chat::with(|c| c.search.count()).as_deref(), Some("No matches"));
        step(true);
        assert_eq!(chat::with(|c| c.search.current()), None);
        set_query(String::new());
        assert_eq!(chat::with(|c| c.search.count()), None);
        close();
        assert!(chat::with(|c| c.search.hits.is_empty() && !c.search.open));
    }

    #[test]
    fn a_hit_inside_a_run_of_tool_calls_opens_that_run_and_is_marked() {
        chat::with(|c| c.session.items = talk());
        open_bar();
        set_query("release".into());
        // The run is items 2 and 3, which start at 2.
        assert!(chat::with(|c| c.open_steps.contains(&2)));
        let marked = chat::with(|c| mark(c, 2, 4, stack()).count());
        let plain = chat::with(|c| mark(c, 0, 2, stack()).count());
        assert!(marked > plain, "the block with the hit has the badge over it");
        // Going to it again leaves it open rather than shutting it.
        step(true);
        assert!(chat::with(|c| c.open_steps.contains(&2)));
        close();
        assert_eq!(chat::with(|c| mark(c, 2, 4, stack()).count()), plain, "closed, nothing is marked");
    }
}
