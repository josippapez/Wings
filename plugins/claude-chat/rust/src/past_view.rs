//! The project's past sessions, in a sheet over the chat as Wings' own Past sessions shows them: searched by their words,
//! narrowed by when they were last active and by branch, each with your first prompt, when and where it ran, and Resume,
//! which goes on with it in this chat.

use std::ops::Range;

use serde_json::Value;
use wings_plugin::{
    format::truncate,
    host,
    local_time::{at, local},
    ui::{Node, badge, button, chips, empty, icon, list_item, muted, notice, row, search_field, select, sheet, stack, text},
};

use crate::chat::{self, Chat, Past};

/// A branch, as Wings' Past sessions draws its filter: Lucide's `git-branch`.
const BRANCH: [&str; 3] = ["M15 6a9 9 0 0 0-9 9V3", "M15 6a3 3 0 1 0 6 0a3 3 0 1 0-6 0", "M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0"];
/// How recently a session was last active, as the sheet's toggle offers it.
const SINCE: [(&str, &str); 4] = [("", "Any time"), ("today", "Today"), ("week", "7 days"), ("month", "30 days")];
const DAY: f64 = 86_400_000.0;
/// The room a row keeps on its right for its Resume button, which shows there while you point at it.
const RESUME_ROOM: u32 = 72;

/// The sheet, while it is open.
pub fn sheet_of(c: &Chat) -> Option<Node> {
    let past = c.past.as_ref()?;
    let search = search_field("Search past sessions", past.query.as_str(), |query| chat::load_past(query.to_owned()))
        .prop("placeholder", "Search prompts, replies, titles, files");
    let since = chips("Last active", &SINCE, past.since.as_str(), chat::set_since);
    let filters = row().prop("gap", "m").prop("align", "center").prop("justify", "between").child(since).maybe(branch_filter(past));
    let now = host::now();
    let shown: Vec<&Value> = past.sessions.iter().flatten().filter(|session| listed(session, past, now)).collect();
    let filtered = !past.query.trim().is_empty() || !past.since.is_empty() || past.branch.is_some();
    let body = match (&past.sessions, &past.error) {
        (_, Some(error)) => notice("danger", error.as_str()),
        (None, None) => reading(),
        (Some(_), None) if shown.is_empty() && filtered => {
            empty("No session matches", "Every word has to appear in the session. Try fewer words, another branch or a longer time.")
        }
        (Some(_), None) if shown.is_empty() => empty("No past sessions yet", "Sessions you run in this project show up here."),
        (Some(_), None) => {
            let words = terms(&past.query);
            stack().prop("gap", "m").children(shown.into_iter().map(|session| session_row(session, &words, now, c)))
        }
    };
    let description = format!("Claude Code sessions in {}.", c.folder);
    Some(sheet("Past sessions", &description, [search, filters], [body], chat::close_past))
}

/// The branch filter, once the sessions name more than one branch.
fn branch_filter(past: &Past) -> Option<Node> {
    let mut branches: Vec<&str> = Vec::new();
    for branch in past.sessions.iter().flatten().filter_map(|session| session["gitBranch"].as_str()) {
        if !branch.is_empty() && !branches.contains(&branch) {
            branches.push(branch);
        }
    }
    if branches.len() < 2 && past.branch.is_none() {
        return None;
    }
    let options: Vec<(String, String)> =
        std::iter::once((String::new(), "All branches".to_owned())).chain(branches.iter().map(|branch| (branch.to_string(), branch.to_string()))).collect();
    let value = past.branch.clone().unwrap_or_default();
    Some(
        row()
            .prop("gap", "xs")
            .prop("align", "center")
            .child(icon(24, 2.0, &BRANCH))
            .child(select("Branch", &options, &value, chat::set_branch).prop("variant", "ghost")),
    )
}

/// Whether `session` is listed under the sheet's time and branch filters, at `now`.
fn listed(session: &Value, past: &Past, now: f64) -> bool {
    let active = session["lastActiveMs"].as_f64().unwrap_or(0.0);
    let since = match past.since.as_str() {
        "today" => {
            let today = local(now as i64);
            at(today.year, today.month, i64::from(today.day), 0, 0) as f64
        }
        "week" => now - 7.0 * DAY,
        "month" => now - 30.0 * DAY,
        _ => 0.0,
    };
    let branch = past.branch.as_deref().is_none_or(|branch| session["gitBranch"].as_str() == Some(branch));
    active >= since && branch
}

/// Grey bars where the sessions will be, while the first search reads the transcripts.
fn reading() -> Node {
    let bar = |height: u32, width: &str| Node::new("Skeleton").prop("height", height).prop("width", width);
    let rows = (0..4).map(|n| stack().prop("gap", "s").key(n.to_string()).child(bar(16, ["70%", "55%", "80%", "60%"][n])).child(bar(12, "90%")).child(bar(12, "25%")));
    stack().prop("gap", "l").children(rows)
}

/// One session: the Claude mark and its title, what you first asked with the searched words marked, when and where it
/// ran, and Resume over its end while you point at it. The session this chat runs is marked instead.
fn session_row(session: &Value, words: &[String], now: f64, c: &Chat) -> Node {
    let id = session["id"].as_str().unwrap_or_default().to_owned();
    let prompt = session["firstPrompt"].as_str().map(one_line).filter(|prompt| !prompt.is_empty());
    let title = session["title"].as_str().map(one_line).filter(|title| !title.is_empty()).or(prompt.clone()).unwrap_or_else(|| "Untitled session".into());
    let current = c.session.id.as_deref() == Some(id.as_str());
    let head = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(Node::new("Icon").prop("name", "claude").prop("size", 12))
        .child(text(title.as_str()).prop("weight", "medium").prop("truncate", true).prop("title", title.as_str()))
        .maybe(current.then(|| badge("This chat", None)));
    // Your first prompt shows when it says more than the title, or when a search may have found words in it.
    let said = prompt.filter(|prompt| !words.is_empty() || *prompt != title).map(|prompt| you_said(&truncate(&prompt, 400), words));
    let facts = muted(meta(session, now, &c.cwd)).prop("size", "small").prop("truncate", true);
    let content = [stack().prop("gap", "xs").prop("grow", true).child(head).maybe(said).child(facts), stack().prop("width", RESUME_ROOM)];
    let resume = button("Resume", "secondary", false, move || chat::resume(&id)).prop("size", "sm").prop("title", "Go on with this session in this chat");
    match current {
        true => Node::new("ListItem").children(content),
        false => list_item(content, resume),
    }
    .key(session["id"].as_str().unwrap_or_default())
}

/// "You: " and the prompt, small and muted, with each word of `words` it holds marked.
fn you_said(prompt: &str, words: &[String]) -> Node {
    let mut line = Node::new("Text").prop("size", "small").prop("clamp", true).child(text("You: "));
    let mut from = 0;
    for found in marks(prompt, words) {
        if found.start > from {
            line = line.child(muted(&prompt[from..found.start]));
        }
        line = line.child(text(&prompt[found.clone()]).prop("mark", true));
        from = found.end;
    }
    if from < prompt.len() {
        line = line.child(muted(&prompt[from..]));
    }
    line
}

/// The line under a session's title, as Past sessions writes it: "5 minutes ago on main in packages/ui · 2 matching
/// messages". The folder shows when it isn't the chat's own: by its path inside it, else by its name.
fn meta(session: &Value, now: f64, cwd: &str) -> String {
    let mut line = ago(session["lastActiveMs"].as_f64().unwrap_or(0.0), now);
    if let Some(branch) = session["gitBranch"].as_str().filter(|branch| !branch.is_empty()) {
        line.push_str(&format!(" on {branch}"));
    }
    let place = session["cwd"].as_str().filter(|folder| *folder != cwd).and_then(|folder| match folder.strip_prefix(&format!("{cwd}/")) {
        Some(inside) => Some(inside.to_owned()),
        None => folder.rsplit('/').find(|part| !part.is_empty()).map(str::to_owned),
    });
    if let Some(place) = place {
        line.push_str(&format!(" in {place}"));
    }
    let matches = session["matches"].as_u64().unwrap_or(0);
    if matches > 1 {
        line.push_str(&format!(" · {matches} matching messages"));
    }
    line
}

/// "5 minutes ago", "yesterday": how Past sessions says when, as `Intl.RelativeTimeFormat` does with `numeric: "auto"`.
fn ago(then: f64, now: f64) -> String {
    let minutes = ((now - then) / 60_000.0).round() as i64;
    let (count, unit) = match minutes {
        ..60 => (minutes, "minute"),
        _ if ((minutes as f64) / 60.0).round() < 24.0 => (((minutes as f64) / 60.0).round() as i64, "hour"),
        _ => (((minutes as f64) / 1440.0).round() as i64, "day"),
    };
    match (count, unit) {
        (..=0, "minute") => "this minute".into(),
        (1, "day") => "yesterday".into(),
        (1, unit) => format!("1 {unit} ago"),
        (count, unit) => format!("{count} {unit}s ago"),
    }
}

/// Text on one line, as a web page shows it: runs of white space are a single space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of a search, lower-cased, each once: a phrase in quotes is one.
fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, part) in query.split('"').enumerate() {
        let words: Vec<&str> = part.split_whitespace().collect();
        let found = if at % 2 == 1 { vec![words.join(" ")] } else { words.into_iter().map(str::to_owned).collect() };
        for term in found.into_iter().filter(|term| !term.is_empty()).map(|term| fold(&term)) {
            if !out.contains(&term) {
                out.push(term);
            }
        }
    }
    out
}

/// `text` lower-cased letter for letter, so a place in it is the same place in `text`.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) if l.len_utf8() == c.len_utf8() => l,
                _ => c,
            }
        })
        .collect()
}

/// Where `words` are in `text`, left to right.
fn marks(text: &str, words: &[String]) -> Vec<Range<usize>> {
    let folded = fold(text);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some((start, len)) = words.iter().filter_map(|word| Some((from + folded[from..].find(word.as_str())?, word.len()))).min_by_key(|(start, _)| *start) {
        found.push(start..start + len);
        from = start + len;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_past_sessions_writes_them() {
        let now = 1_000_000_000_000.0;
        let minutes = |n: f64| now - n * 60_000.0;
        assert_eq!(ago(now, now), "this minute");
        assert_eq!(ago(minutes(1.0), now), "1 minute ago");
        assert_eq!(ago(minutes(5.0), now), "5 minutes ago");
        assert_eq!(ago(minutes(60.0), now), "1 hour ago");
        assert_eq!(ago(minutes(180.0), now), "3 hours ago");
        assert_eq!(ago(minutes(24.0 * 60.0), now), "yesterday");
        assert_eq!(ago(minutes(5.0 * 24.0 * 60.0), now), "5 days ago");
    }

    #[test]
    fn the_searched_words_are_marked_wherever_they_are_whatever_their_case() {
        let words = terms("login \"sign in\" Login");
        assert_eq!(words, ["login", "sign in"]);
        let text = "Fix the Login redirect, then sign in again";
        let found: Vec<&str> = marks(text, &words).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["Login", "sign in"]);
        assert!(marks(text, &[]).is_empty());
    }

    #[test]
    fn a_sessions_line_says_when_where_and_how_many_messages_matched() {
        let now = 1_000_000_000_000.0;
        let session = |cwd: &str, matches: u64| {
            serde_json::json!({ "lastActiveMs": now - 5.0 * 60_000.0, "gitBranch": "main", "cwd": cwd, "matches": matches })
        };
        assert_eq!(meta(&session("/work/shop", 1), now, "/work/shop"), "5 minutes ago on main");
        assert_eq!(meta(&session("/work/shop/packages/ui", 2), now, "/work/shop"), "5 minutes ago on main in packages/ui · 2 matching messages");
        assert_eq!(meta(&session("/work/other", 0), now, "/work/shop"), "5 minutes ago on main in other");
    }
}
