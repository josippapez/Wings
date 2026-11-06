//! What the command palette shows for what you type: the desktop's `lib/palette.ts`. Every word of the query has to match,
//! in the title or, at half weight, in the subtitle and keywords. A run of the word beats letters spread over the text,
//! and the start of the text or of a word beats the middle.

use crate::agents::AgentState;

/// Where an item comes from. The order is the order of the groups when nothing is typed, and the tie-break when
/// something is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Agent,
    Command,
    Project,
    Tab,
    Session,
    Plugin,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Agent => "Agents",
            Kind::Command => "Commands",
            Kind::Project => "Projects",
            Kind::Tab => "Tabs",
            Kind::Session => "Recent sessions",
            Kind::Plugin => "Plugins",
        }
    }
}

/// One line of the palette. `R` is what choosing it does.
#[derive(Clone, Debug)]
pub(super) struct Item<R> {
    pub kind: Kind,
    pub title: String,
    /// Shown beside the title, and searched at half weight.
    pub subtitle: Option<String>,
    /// More words to find it by.
    pub keywords: Option<String>,
    pub shortcut: Option<String>,
    /// An agent's state, drawn as its dot.
    pub state: Option<AgentState>,
    pub disabled: bool,
    pub run: R,
}

impl<R> Item<R> {
    pub fn new(kind: Kind, title: impl Into<String>, run: R) -> Self {
        Self {
            kind,
            title: title.into(),
            subtitle: None,
            keywords: None,
            shortcut: None,
            state: None,
            disabled: false,
            run,
        }
    }
}

fn is_word_start(text: &[char], at: usize) -> bool {
    at == 0
        || text
            .get(at - 1)
            .is_some_and(|c| c.is_whitespace() || "-_/.:·".contains(*c))
}

/// How well `token` matches `text`, or `None` if it doesn't. Both lowercase.
fn token_score(token: &str, text: &str) -> Option<f64> {
    let text: Vec<char> = text.chars().collect();
    let token: Vec<char> = token.chars().collect();
    if let Some(at) = text
        .windows(token.len().max(1))
        .position(|window| window == token.as_slice())
    {
        // A run of the word, ranked by where it sits: the whole text, the start of it, the start of a word, the middle.
        let base = if at == 0 {
            if token.len() == text.len() {
                300.0
            } else {
                200.0
            }
        } else if is_word_start(&text, at) {
            150.0
        } else {
            100.0
        };
        return Some(
            base - at.min(40) as f64 * 0.5 - (text.len() - token.len()).min(60) as f64 * 0.1,
        );
    }
    // Letters in order: each one scores more after the one before it or at the start of a word.
    let mut score = 10.0;
    let mut from = 0;
    let mut previous: Option<usize> = None;
    for ch in token {
        let found = text[from.min(text.len())..].iter().position(|c| *c == ch)? + from;
        score += if previous.is_some_and(|p| found == p + 1) {
            6.0
        } else if is_word_start(&text, found) {
            4.0
        } else {
            1.0 - (found - from).min(10) as f64 * 0.2
        };
        previous = Some(found);
        from = found + 1;
    }
    Some(score)
}

/// Every word of the query has to match, in the title or (at half weight) the subtitle and keywords.
fn match_score<R>(query: &str, item: &Item<R>) -> Option<f64> {
    let query = query.to_lowercase();
    let title = item.title.to_lowercase();
    let rest = format!(
        "{} {}",
        item.subtitle.as_deref().unwrap_or(""),
        item.keywords.as_deref().unwrap_or("")
    )
    .to_lowercase();
    let mut total = 0.0;
    for token in query.split_whitespace() {
        let in_title = token_score(token, &title);
        let in_rest = token_score(token, &rest).map(|score| score / 2.0);
        total += match (in_title, in_rest) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => return None,
        };
    }
    Some(total)
}

/// An agent that needs you outranks one that finished, then one that is working, then an idle one.
fn need<R>(item: &Item<R>) -> i32 {
    match (item.kind, item.state) {
        (Kind::Agent, Some(AgentState::Blocked)) => 3,
        (Kind::Agent, Some(AgentState::Done)) => 2,
        (Kind::Agent, Some(AgentState::Working)) => 1,
        (Kind::Agent, Some(AgentState::Idle)) => 0,
        _ => -1,
    }
}

/// The heading an item sits under when nothing is typed. An agent that needs you is in its own group at the top.
pub(super) fn group_of<R>(item: &Item<R>) -> &'static str {
    if need(item) >= 2 {
        "Needs you"
    } else {
        item.kind.label()
    }
}

const KINDS: [Kind; 6] = [
    Kind::Agent,
    Kind::Command,
    Kind::Project,
    Kind::Tab,
    Kind::Session,
    Kind::Plugin,
];

fn kind_order(kind: Kind) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(KINDS.len())
}

/// The indices of the items to show. Nothing typed: the agents that need you, then each kind in turn in the order
/// given. Something typed: the items that match, best first, with an agent that needs you ahead of equal matches.
pub(super) fn rank<R>(items: &[Item<R>], query: &str, limit: usize) -> Vec<usize> {
    let query = query.trim();
    let mut shown: Vec<usize>;
    if query.is_empty() {
        let (mut waiting, mut rest): (Vec<usize>, Vec<usize>) =
            (0..items.len()).partition(|&i| need(&items[i]) >= 2);
        waiting.sort_by_key(|&i| std::cmp::Reverse(need(&items[i])));
        rest.sort_by_key(|&i| kind_order(items[i].kind));
        waiting.extend(rest);
        shown = waiting;
    } else {
        let mut scored: Vec<(usize, f64)> = items
            .iter()
            .enumerate()
            .filter_map(|(at, item)| {
                Some((at, match_score(query, item)? + f64::from(need(item)) * 8.0))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then_with(|| kind_order(items[a.0].kind).cmp(&kind_order(items[b.0].kind)))
                .then_with(|| a.0.cmp(&b.0))
        });
        shown = scored.into_iter().map(|(at, _)| at).collect();
    }
    shown.truncate(limit);
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, title: &str) -> Item<()> {
        Item::new(kind, title, ())
    }

    fn titles(items: &[Item<()>], query: &str) -> Vec<String> {
        rank(items, query, 80)
            .into_iter()
            .map(|i| items[i].title.clone())
            .collect()
    }

    #[test]
    fn a_run_in_the_title_beats_letters_spread_over_it() {
        let items = [
            item(Kind::Command, "Split Down"),
            item(Kind::Command, "Toggle Sidebar"),
            item(Kind::Command, "Settings"),
        ];
        assert_eq!(titles(&items, "sid"), ["Toggle Sidebar", "Split Down"]);
        assert_eq!(titles(&items, "set"), ["Settings"]);
        assert!(titles(&items, "zzz").is_empty());
    }

    #[test]
    fn every_word_has_to_match_and_the_subtitle_counts_for_less() {
        let mut web = item(Kind::Project, "acme-web");
        web.subtitle = Some("~/code/acme-web".into());
        let mut api = item(Kind::Project, "acme-api");
        api.keywords = Some("backend".into());
        let items = [web, api];
        assert_eq!(titles(&items, "acme api"), ["acme-api"]);
        assert_eq!(titles(&items, "backend"), ["acme-api"]);
        assert_eq!(titles(&items, "code"), ["acme-web"]);
        assert!(titles(&items, "acme backend web").is_empty());
    }

    #[test]
    fn nothing_typed_lists_agents_that_need_you_then_each_kind_in_turn() {
        let agent = |title: &str, state| {
            let mut item = item(Kind::Agent, title);
            item.state = Some(state);
            item
        };
        let items = [
            item(Kind::Tab, "Terminal"),
            agent("Working one", AgentState::Working),
            item(Kind::Command, "New Tab"),
            agent("Done one", AgentState::Done),
            agent("Blocked one", AgentState::Blocked),
            item(Kind::Project, "acme-web"),
        ];
        assert_eq!(
            titles(&items, "  "),
            [
                "Blocked one",
                "Done one",
                "Working one",
                "New Tab",
                "acme-web",
                "Terminal"
            ]
        );
        let groups: Vec<_> = rank(&items, "", 80)
            .into_iter()
            .map(|i| group_of(&items[i]))
            .collect();
        assert_eq!(
            groups,
            [
                "Needs you",
                "Needs you",
                "Agents",
                "Commands",
                "Projects",
                "Tabs"
            ]
        );
    }

    #[test]
    fn an_agent_that_needs_you_wins_a_tie() {
        let mut waiting = item(Kind::Agent, "Fix the login");
        waiting.state = Some(AgentState::Blocked);
        let items = [item(Kind::Tab, "Fix the login"), waiting];
        assert_eq!(rank(&items, "fix", 80), [1, 0]);
    }

    #[test]
    fn the_list_is_cut_to_the_limit() {
        let items: Vec<_> = (0..10)
            .map(|n| item(Kind::Command, &format!("Command {n}")))
            .collect();
        assert_eq!(rank(&items, "", 3).len(), 3);
        assert_eq!(rank(&items, "command", 4).len(), 4);
    }
}
