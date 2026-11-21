//! Plans as documents. Every plan Claude proposes with ExitPlanMode stays in the chat after you approve or reject it:
//! the side panel lists them, and each opens as a page of Markdown (`plan_doc_view.rs`) where a part can be commented.
//! A comment is a `review::Comment` on the path `plan #<n>`, so it waits above the message box, is kept with the tab
//! and goes with your next message like a comment from the diff viewer does, under its own heading. While the plan
//! still waits for an answer, "Revise with comments" sends them as the reason for the no.
//!
//! The plans are read from the calls Claude makes (live, and from the transcript of a resumed session, which goes
//! through the same code) and from the question it asks. Claude Code's `get_plan` request, which answers with the
//! plan file of the session, refreshes the newest.

use serde_json::{Value, json};
use wings_plugin::format::truncate;

use crate::{
    review::Comment,
    session::{Ask, Item},
};

/// The id of the `get_plan` request.
pub const REFRESH: &str = "plan-refresh";
/// The most plans a chat keeps.
const MAX_PLANS: usize = 50;
/// The start of the path a comment on a plan has, followed by the plan's number.
pub const PATH: &str = "plan #";
/// How much of a block a comment quotes.
const QUOTE: usize = 600;

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The id of the call that proposed it (`ask:<request>` for one known only from the question).
    pub id: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Plans {
    list: Vec<Plan>,
    /// A `get_plan` is on its way.
    pub refreshing: bool,
    /// What the last refresh found, when it found no plan.
    pub note: Option<String>,
}

/// How a plan stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Approved,
    Rejected,
    /// The call has no answer and no question waits: the turn was stopped, or it is not known.
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Waiting => "Waiting for approval",
            Self::Approved => "Approved",
            Self::Rejected => "Not approved",
            Self::Unknown => "Proposed",
        }
    }
}

impl Plans {
    /// Takes in the plan of an ExitPlanMode call `id`, once.
    pub fn seen(&mut self, id: &str, input: &Value) {
        let Some(text) = input["plan"].as_str().filter(|text| !text.trim().is_empty()) else {
            return;
        };
        match self.list.iter_mut().find(|plan| plan.id == id) {
            Some(plan) => plan.text = text.to_owned(),
            None => {
                self.list.push(Plan { id: id.to_owned(), text: text.to_owned() });
                if self.list.len() > MAX_PLANS {
                    self.list.remove(0);
                }
            }
        }
    }

    /// Takes in the plan of a question Claude Code asks for `request`, which names its call as `call`. Without the call's
    /// name, the question of a plan the last call already brought adds nothing.
    pub fn asked(&mut self, request: &str, call: Option<&str>, input: &Value) {
        match call {
            Some(call) => self.seen(call, input),
            None if self.list.last().is_some_and(|plan| Some(plan.text.as_str()) == input["plan"].as_str()) => {}
            None => self.seen(&format!("ask:{request}"), input),
        }
    }

    /// The plans that are still part of the conversation (a rewind drops a call, and its plan with it), newest first, each
    /// with its number: the order they were proposed in, from 1.
    pub fn shown(&self, items: &[Item]) -> Vec<(usize, &Plan)> {
        let live = |plan: &&Plan| plan.id.starts_with("ask:") || items.iter().any(|item| matches!(item, Item::Tool(tool) if tool.id == plan.id)) || waiting(items, plan).is_some();
        let mut shown: Vec<(usize, &Plan)> = self.list.iter().filter(live).enumerate().map(|(at, plan)| (at + 1, plan)).collect();
        shown.reverse();
        shown
    }

    /// The line that asks Claude Code for the plan file with `get_plan`. None while one is on its way.
    pub fn refresh_line(&mut self) -> Option<String> {
        if std::mem::replace(&mut self.refreshing, true) {
            return None;
        }
        self.note = None;
        Some(crate::session::request(REFRESH, json!({ "subtype": "get_plan" })))
    }

    /// Claude Code's answer to `get_plan` (`{ exists, content?, path? }`): the plan file replaces the text of the newest plan,
    /// or is the only plan when none was proposed here. Whether `id` was this request.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        if id != REFRESH {
            return false;
        }
        self.refreshing = false;
        let answer = &response["response"];
        match (response["subtype"] == "success", answer["exists"] == true, answer["content"].as_str().filter(|text| !text.trim().is_empty())) {
            (true, true, Some(content)) => {
                match self.list.last_mut() {
                    Some(plan) => plan.text = content.to_owned(),
                    None => self.list.push(Plan { id: "ask:current".into(), text: content.to_owned() }),
                }
                self.note = None;
            }
            (true, ..) => self.note = Some("Claude Code has no plan file for this session yet.".into()),
            _ => self.note = Some(format!("Claude Code didn't send the plan: {}", response["error"].as_str().unwrap_or("no reason given"))),
        }
        true
    }
}

/// The id of the question that waits for an answer to `plan`.
pub fn waiting<'a>(items: &'a [Item], plan: &Plan) -> Option<&'a str> {
    items.iter().find_map(|item| match item {
        Item::Ask(Ask { tool, input, answer: None, request, .. }) if tool.as_str() == "ExitPlanMode" && input["plan"].as_str() == Some(plan.text.as_str()) => Some(request.as_str()),
        _ => None,
    })
}

/// How `plan` stands: what the call printed once it was answered, else whether a question waits.
pub fn status(items: &[Item], plan: &Plan) -> Status {
    if waiting(items, plan).is_some() {
        return Status::Waiting;
    }
    match items.iter().find_map(|item| match item {
        Item::Tool(tool) if tool.id == plan.id => Some(&tool.output),
        _ => None,
    }) {
        Some(Some((_, false))) => Status::Approved,
        Some(Some((_, true))) => Status::Rejected,
        _ => Status::Unknown,
    }
}

/// A plan's name: its first line, without the Markdown mark of a heading.
pub fn title(plan: &Plan) -> String {
    let first = plan.text.lines().map(|line| line.trim().trim_start_matches('#').trim()).find(|line| !line.is_empty());
    truncate(first.unwrap_or("Plan"), 80)
}

/// What the plan page keeps while it is open.
#[derive(Debug, Default)]
pub struct View {
    /// The plan open as a document, by its id.
    pub open: Option<String>,
    /// The block (by its first line) a comment is being written on, and what is typed.
    pub editing: Option<u64>,
    pub draft: String,
}

// ---------- what you can comment on ----------

/// A part of a plan that takes a comment: a heading, a list item with what is indented under it, a paragraph, a table or
/// a code block, by the plan's lines (from 1).
#[derive(Debug, PartialEq)]
pub struct Block {
    pub start: u64,
    pub end: u64,
    pub text: String,
}

impl Block {
    /// What a Markdown renderer is given: the block without the indent of its first line, which would make a nested list
    /// item a code block on its own.
    pub fn source(&self) -> String {
        let indent = self.text.len() - self.text.trim_start_matches(' ').len();
        self.text
            .lines()
            .map(|line| {
                let strip = line.len() - line.trim_start_matches(' ').len();
                &line[strip.min(indent)..]
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn heading(line: &str) -> bool {
    let rest = line.trim_start_matches('#');
    line.len() - rest.len() <= 6 && line.starts_with('#') && (rest.is_empty() || rest.starts_with(' '))
}

fn list_item(line: &str) -> bool {
    if ["- ", "* ", "+ "].iter().any(|mark| line.starts_with(mark)) {
        return true;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && line[digits..].starts_with(['.', ')']) && line[digits + 1..].starts_with(' ')
}

/// A code fence's opening: its character and length.
fn fence(line: &str) -> Option<(char, usize)> {
    let mark = line.chars().next().filter(|mark| matches!(mark, '`' | '~'))?;
    let len = line.chars().take_while(|c| *c == mark).count();
    (len >= 3).then_some((mark, len))
}

/// The parts of `plan` that take a comment. Blank lines separate them, as does each heading and each list item, and a
/// code block stays whole, its blank lines too.
pub fn blocks(plan: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut current: Option<Block> = None;
    let mut is_heading = false;
    let mut open: Option<(char, usize)> = None;
    let mut flush = |current: &mut Option<Block>| out.extend(current.take());
    for (at, line) in plan.lines().enumerate() {
        let n = at as u64 + 1;
        let trimmed = line.trim_start();
        if let Some((mark, len)) = open {
            if let Some(block) = current.as_mut() {
                block.text.push('\n');
                block.text.push_str(line);
                block.end = n;
            }
            if fence(trimmed).is_some_and(|(close, run)| close == mark && run >= len) && trimmed.trim_start_matches(mark).trim().is_empty() {
                open = None;
                flush(&mut current);
            }
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut current);
            continue;
        }
        let opens = fence(trimmed);
        let starts = opens.is_some() || heading(trimmed) || list_item(trimmed);
        match current.as_mut().filter(|_| !starts && !is_heading) {
            Some(block) => {
                block.text.push('\n');
                block.text.push_str(line);
                block.end = n;
            }
            None => {
                flush(&mut current);
                current = Some(Block { start: n, end: n, text: line.to_owned() });
                is_heading = heading(trimmed);
                open = opens;
            }
        }
    }
    flush(&mut current);
    out
}

// ---------- the comments ----------

/// The path of the comments on plan number `number`.
pub fn path(number: usize) -> String {
    format!("{PATH}{number}")
}

/// Whether a comment's path is a plan's.
pub fn is_plan(path: &str) -> bool {
    path.starts_with(PATH)
}

/// Your `body` on `block` of plan `number`, made at `stamp` (ms). None for an empty one.
pub fn comment(number: usize, block: &Block, body: &str, stamp: u64) -> Option<Comment> {
    let body = body.trim();
    (!body.is_empty()).then(|| Comment {
        id: format!("plan-{number}-{}-{stamp}", block.start),
        path: path(number),
        old: false,
        line: block.start,
        end_line: (block.end > block.start).then_some(block.end),
        quote: truncate(block.text.trim_end(), QUOTE),
        body: body.to_owned(),
    })
}

/// The comments on plans, taken out of `comments`: those for the answer to a plan. The rest stay.
pub fn take(comments: &mut Vec<Comment>) -> Vec<Comment> {
    let (plans, rest): (Vec<_>, Vec<_>) = std::mem::take(comments).into_iter().partition(|comment| is_plan(&comment.path));
    *comments = rest;
    plans
}

/// How many comments wait on plans.
pub fn count(comments: &[Comment]) -> usize {
    comments.iter().filter(|comment| is_plan(&comment.path)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Detail, Tool};

    fn tool(id: &str, output: Option<(&str, bool)>) -> Item {
        Item::Tool(Tool { id: id.into(), name: "ExitPlanMode".into(), summary: String::new(), output: output.map(|(said, failed)| (said.into(), failed)), detail: Detail::Plain, input: Default::default(), images: Vec::new() })
    }

    fn ask(request: &str, plan: &str, answer: Option<bool>) -> Item {
        Item::Ask(Ask {
            request: request.into(),
            tool: "ExitPlanMode".into(),
            summary: String::new(),
            detail: String::new(),
            cut_short: false,
            reason: None,
            suggestions: Vec::new(),
            input: json!({ "plan": plan }),
            preview: Default::default(),
            answer,
        })
    }

    fn spans(plan: &str) -> Vec<(u64, u64)> {
        blocks(plan).iter().map(|block| (block.start, block.end)).collect()
    }

    #[test]
    fn a_plan_splits_into_headings_list_items_paragraphs_and_whole_code_blocks() {
        let plan = "## Split it\n\nSome words\nover two lines.\n\n1. Move `src`\n   to the package\n2. Add tests\n   - nested\n\n```sh\npnpm test\n\npnpm build\n```\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        assert_eq!(spans(plan), [(1, 1), (3, 4), (6, 7), (8, 8), (9, 9), (11, 15), (16, 18)]);
        let all = blocks(plan);
        assert_eq!(all[2].text, "1. Move `src`\n   to the package");
        // Text right under a heading is a block of its own.
        assert_eq!(spans("# A\ntext\ntext"), [(1, 1), (2, 3)]);
        // A dash that isn't a list mark, and an empty plan.
        assert_eq!(spans("---\n**bold** line"), [(1, 2)]);
        assert!(blocks("").is_empty());
        // A fence that is never closed holds the rest.
        assert_eq!(spans("```\na\n\nb"), [(1, 4)]);
    }

    #[test]
    fn a_nested_item_is_rendered_without_the_indent_that_would_make_it_code() {
        let all = blocks("- a\n      - deep\n      more");
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].source(), "- deep\nmore");
        assert_eq!(blocks("1. x\n   y")[0].source(), "1. x\n   y");
    }

    #[test]
    fn plans_come_from_the_calls_once_and_the_question_adds_one_only_when_it_is_new() {
        let mut plans = Plans::default();
        plans.seen("t1", &json!({ "plan": "# One" }));
        plans.seen("t1", &json!({ "plan": "# One, edited" }));
        plans.seen("t2", &json!({ "plan": "  " }));
        plans.seen("t2", &json!({ "allowedPrompts": [] }));
        // The question for a call already known adds nothing, with or without its call's id.
        plans.asked("r1", Some("t1"), &json!({ "plan": "# One, edited" }));
        plans.asked("r1", None, &json!({ "plan": "# One, edited" }));
        let items = [tool("t1", None)];
        assert_eq!(plans.shown(&items).iter().map(|(n, plan)| (*n, plan.text.as_str())).collect::<Vec<_>>(), [(1, "# One, edited")]);
        // A question alone is a plan of its own, shown with no call.
        plans.asked("r2", None, &json!({ "plan": "# Two" }));
        let shown = plans.shown(&items);
        assert_eq!(shown.iter().map(|(n, plan)| (*n, title(plan))).collect::<Vec<_>>(), [(2, "Two".to_owned()), (1, "One, edited".to_owned())]);
    }

    #[test]
    fn a_plan_whose_call_a_rewind_dropped_is_gone_and_the_numbers_of_the_rest_stay() {
        let mut plans = Plans::default();
        plans.seen("t1", &json!({ "plan": "one" }));
        plans.seen("t2", &json!({ "plan": "two" }));
        assert_eq!(plans.shown(&[tool("t1", None)]).len(), 1);
        assert_eq!(plans.shown(&[tool("t1", None), tool("t2", None)])[0].0, 2);
    }

    #[test]
    fn a_plan_is_waiting_while_a_question_asks_then_approved_or_not_as_the_call_ended() {
        let plan = Plan { id: "t1".into(), text: "# One".into() };
        let pending = [tool("t1", None), ask("r1", "# One", None)];
        assert_eq!(status(&pending, &plan), Status::Waiting);
        assert_eq!(waiting(&pending, &plan), Some("r1"));
        assert_eq!(status(&[tool("t1", Some(("User has approved your plan", false))), ask("r1", "# One", Some(true))], &plan), Status::Approved);
        assert_eq!(status(&[tool("t1", Some(("No", true)))], &plan), Status::Rejected);
        assert_eq!(status(&[tool("t1", None)], &plan), Status::Unknown);
        assert_eq!(waiting(&[ask("r1", "# Other", None)], &plan), None);
    }

    #[test]
    fn get_plan_replaces_the_newest_plan_and_says_when_there_is_none() {
        let mut plans = Plans::default();
        let line: Value = serde_json::from_str(plans.refresh_line().unwrap().trim()).unwrap();
        assert_eq!((line["request_id"].as_str(), line["request"]["subtype"].as_str()), (Some("plan-refresh"), Some("get_plan")));
        // Not asked twice while it waits.
        assert!(plans.refresh_line().is_none());
        assert!(!plans.answered("other", &json!({})));
        let none = json!({ "subtype": "success", "response": { "exists": false } });
        assert!(plans.answered(REFRESH, &none));
        assert!(!plans.refreshing && plans.note.is_some());
        // With no plan proposed here, the file is the plan.
        let file = json!({ "subtype": "success", "response": { "exists": true, "content": "# From the file", "path": "/p.md" } });
        assert!(plans.answered(REFRESH, &file));
        assert_eq!(plans.shown(&[]).iter().map(|(_, plan)| plan.text.as_str()).collect::<Vec<_>>(), ["# From the file"]);
        plans.seen("t1", &json!({ "plan": "# Newer" }));
        let edited = json!({ "subtype": "success", "response": { "exists": true, "content": "# Newer, edited" } });
        plans.answered(REFRESH, &edited);
        assert_eq!(plans.shown(&[tool("t1", None)])[0].1.text, "# Newer, edited");
        assert!(plans.note.is_none());
        let failed = json!({ "subtype": "error", "error": "no" });
        plans.answered(REFRESH, &failed);
        assert_eq!(plans.note.as_deref(), Some("Claude Code didn't send the plan: no"));
    }

    #[test]
    fn a_comment_quotes_its_block_and_is_taken_with_the_plan_ones_only() {
        let all = blocks("# A\n\n1. one\n   more");
        let item = comment(2, &all[1], "  Why?  ", 7).unwrap();
        assert_eq!((item.path.as_str(), item.line, item.end_line, item.body.as_str()), ("plan #2", 3, Some(4), "Why?"));
        assert_eq!(item.quote, "1. one\n   more");
        assert_eq!(item.place(), "plan #2:3-4");
        assert!(comment(2, &all[0], "   ", 7).is_none());
        let other = Comment { path: "src/a.rs".into(), ..item.clone() };
        let mut list = vec![other.clone(), item.clone()];
        assert_eq!(count(&list), 1);
        assert_eq!(take(&mut list), [item]);
        assert_eq!(list, [other]);
    }
}
