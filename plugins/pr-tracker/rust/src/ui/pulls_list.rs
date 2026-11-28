//! The project's open pull requests as a list: every open one, with chips to narrow it to yours, the ones waiting for
//! your review or drafts. The Pull requests block of a Project page shows it, and so does a "pr" tab opened with no pull
//! request. Main owns the lists and their cache: this asks it, while Wings is in front, and after a sign-in.

use crate::{
    ipc,
    pulls::{Agent, Filter, PullRow, PullsAnswer, Scope, count_pulls, filter_of, filter_pulls, split_title},
    types::PrArgs,
    ui::parts::{status_badge, tag, time_text},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wings_plugin::{
    host, messages, poll,
    task::spawn,
    ui::{Node, button, chips, empty, icon_button, icons, joined, list_button, muted, notice, page_with, row, rows_card, stack, text, title_with_context, toolbar_titled},
};

/// How often the list is asked for again while Wings is in front.
const REFETCH_MS: u32 = 30_000;
/// Where a tab keeps its filter: a block keeps its own with the block.
const TAB_FILTER_KEY: &str = "pulls-tab-filter";

#[derive(Clone, Copy, PartialEq)]
enum Store {
    Tab,
    Block,
}

struct State {
    store: Store,
    /// Until the app says which project it is, nothing; then the project's folder and name, or none when none is open.
    project: Option<Option<(String, String)>>,
    answer: Option<Result<PullsAnswer, String>>,
    filter: Filter,
    loading: bool,
    signing: bool,
    sign_error: Option<String>,
    /// Goes up with each read: an older one that finds a later number drops out.
    reads: u64,
    on_open: Option<Rc<dyn Fn(PrArgs)>>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State { store: Store::Block, project: None, answer: None, filter: Filter::All, loading: false, signing: false, sign_error: None, reads: 0, on_open: None })
    };
}

fn with<T>(change: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| change(&mut state.borrow_mut()))
}

/// Asks main for the project's list. The answer on screen stays until the new one comes, unless `clear`.
fn read(clear: bool) {
    let Some((path, number)) = with(|s| {
        let Some(Some((path, _))) = &s.project else { return None };
        if clear {
            s.answer = None;
        }
        s.loading = true;
        s.reads += 1;
        Some((path.clone(), s.reads))
    }) else {
        return;
    };
    spawn(async move {
        let asked = messages::ask(ipc::PULLS, json!({ "path": path })).await.and_then(|value| serde_json::from_value::<PullsAnswer>(value).map_err(|error| error.to_string()));
        with(|s| {
            if s.reads == number {
                s.answer = Some(asked);
                s.loading = false;
            }
        });
    });
}

/// The button of a problem, or Check again: what the pane badge's button does for this project's host. A sign-in waits
/// for the browser, so the button is busy meanwhile.
fn act() {
    let Some(path) = with(|s| match &s.project {
        Some(Some((path, _))) if !s.signing => {
            s.signing = true;
            s.sign_error = None;
            Some(path.clone())
        }
        _ => None,
    }) else {
        return;
    };
    spawn(async move {
        let done = messages::ask(ipc::PULLS_ACTION, json!({ "path": path })).await;
        with(|s| {
            s.signing = false;
            s.sign_error = done.err();
        });
        read(false);
    });
}

fn set_filter(filter: Filter) {
    let store = with(|s| {
        s.filter = filter;
        s.store
    });
    let saved = json!({ "filter": filter.name() });
    match store {
        Store::Block => drop(host::set_block_state(&saved)),
        Store::Tab => {
            host::storage::set(TAB_FILTER_KEY, &saved).ok();
        }
    }
}

/// Sets the list up in this frame: the project on screen, the filter it was left on, and the reads while Wings is in front.
fn start(store: Store, on_open: impl Fn(PrArgs) + 'static) {
    with(|s| {
        s.store = store;
        s.on_open = Some(Rc::new(on_open));
    });
    host::on("project", |_, project| {
        if moved_to(project) {
            read(true);
        }
    });
    match store {
        Store::Block => spawn(async {
            let saved = host::block_state().await.unwrap_or(Value::Null);
            with(|s| s.filter = filter_of(&saved));
        }),
        Store::Tab => {
            let saved = host::storage::get(TAB_FILTER_KEY).unwrap_or(Value::Null);
            with(|s| s.filter = filter_of(&saved));
        }
    }
    poll::while_focused(REFETCH_MS, false, || read(false));
    messages::hear(ipc::REFRESH, |_| read(false));
}

/// The block's list: the page of the Pull requests block, which opens a pull request in a tab.
pub fn start_block() {
    start(Store::Block, |args| drop(host::open_view_with("pr", "tab", &serde_json::to_value(&args).unwrap_or(Value::Null))));
}

/// The picker a PR tab with no PR shows: the open pull requests of the project on screen, as a whole page. `pick` hears
/// the `PrArgs` of the one clicked. Call `start_picker` once from the page's `start`, and `picker_view` from its view.
pub fn start_picker(pick: impl Fn(PrArgs) + 'static) {
    start(Store::Tab, pick);
}

const FILTERS: [(Filter, &str); 4] = [(Filter::All, "All open"), (Filter::Mine, "Mine"), (Filter::Review, "Review requested"), (Filter::Drafts, "Drafts")];

fn empty_title(filter: Filter) -> &'static str {
    match filter {
        Filter::All => "No open pull requests",
        Filter::Mine => "None of the open pull requests are yours",
        Filter::Review => "Nobody has asked for your review",
        Filter::Drafts => "No draft pull requests",
    }
}

/// A row's status dot: green unless the tone says more.
fn dot_tone(tone: &str) -> Option<&'static str> {
    match tone {
        "neutral" => Some("muted"),
        "info" | "merged" => Some("info"),
        "warning" => Some("warn"),
        "danger" => Some("danger"),
        _ => None,
    }
}

fn skeletons() -> Node {
    stack().prop("gap", "s").child(Node::new("Spinner").prop("label", "Loading pull requests")).children((0..3).map(|i| Node::new("Skeleton").key(i.to_string()).prop("height", 58)))
}

/// A row: a lead dot, the title over its identity (number, author, branches) and its status, all muted and split by
/// " · ", and the flags at the end. Rows sit in one card split by hairlines.
fn pull_item(r: &PullRow, show_scope: bool) -> Node {
    let (tags, title) = split_title(&r.title);
    let checks = &r.checks;
    let checks_text = if checks.failed > 0 {
        Some(format!("{} failing", checks.failed))
    } else if checks.running > 0 {
        Some(format!("{} running", checks.running))
    } else if checks.queued > 0 {
        Some(format!("{} queued", checks.queued))
    } else if checks.passed > 0 {
        Some(format!("{} passed", checks.passed))
    } else {
        None
    };
    let mut dot = Node::new("Dot").prop("label", r.badge.text.as_str());
    if let Some(tone) = dot_tone(&r.badge.tone) {
        dot = dot.prop("tone", tone);
    }
    let scope = match r.scope {
        Scope::Mine => "Yours",
        Scope::Review => "Review requested",
        Scope::Other => "",
    };
    let args = r.args.clone();
    // The page is being built under a shared borrow of the state, so this is a shared one too.
    let on_open = STATE.with(|s| s.borrow().on_open.clone());
    let title_line = row().prop("gap", "s").prop("align", "center").child(dot).children(tags.iter().enumerate().map(|(i, t)| tag(t, false).key(format!("t{i}")))).child(text(title).prop("weight", "medium").prop("truncate", true).prop("grow", true));
    let small = |t: String| muted(t).prop("size", "small");
    let identity = joined(
        [Some(small(r.number.clone())), r.author.clone().map(small), (!r.branch.is_empty()).then(|| small(r.branch.clone()).prop("mono", true).prop("truncate", true))].into_iter().flatten().collect(),
    );
    let facts = joined(
        [
            (!r.review.is_empty()).then(|| small(r.review.clone())),
            checks_text.map(small),
            time_text(&r.updated).map(small),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    // The status is a filled badge, the same one the pull request page shows; the dot only leads the row.
    let flags: Vec<Node> = [
        (!r.badge.text.is_empty()).then(|| status_badge(r.badge.text.as_str(), &r.badge.tone).prop("size", "sm")),
        (show_scope && !scope.is_empty()).then(|| Node::new("Badge").prop("size", "sm").prop("variant", "outline").text(scope)),
        r.agent.map(|agent| Node::new("Badge").prop("size", "sm").prop("variant", "outline").prop("tone", "info").text(if agent == Agent::Working { "Agent working" } else { "Agent here" })),
        (r.unread > 0).then(|| Node::new("Badge").prop("size", "sm").prop("variant", "outline").prop("tone", "info").text(format!("{} new", r.unread))),
    ]
    .into_iter()
    .flatten()
    .enumerate()
    .map(|(i, badge)| badge.key(format!("f{i}")))
    .collect();
    // The dot leads the title line, so it lines up with the title and not with the middle of the three lines.
    let content = [stack().prop("gap", "xs").prop("grow", true).child(title_line).child(identity).child(facts)]
        .into_iter()
        .chain((!flags.is_empty()).then(|| row().prop("gap", "s").prop("wrap", true).prop("justify", "end").children(flags)));
    list_button(content, move || {
        if let Some(on_open) = &on_open {
            on_open(args.clone());
        }
    })
    .key(r.id.clone())
}

/// What the list says in place of rows: the error with Try again, the host nothing recognised, a problem with its
/// button, or the empty list.
fn body(s: &State, rows: &[PullRow]) -> Node {
    let failed = s.sign_error.as_ref().map(|error| notice("danger", error.as_str()));
    let action = |label: &str| row().child(button(label, "default", s.signing, act));
    match &s.answer {
        Some(Err(error)) => stack().prop("gap", "m").child(notice("danger", error.as_str())).child(row().child(button("Try again", "outline", s.loading, || read(false)))),
        None => skeletons(),
        Some(Ok(answer)) if answer.none == Some(true) => match &answer.host {
            Some(host) => stack()
                .prop("gap", "s")
                .child(muted(format!("No pull request provider found for {host}. For GitHub Enterprise or self-managed GitLab, sign in to it with gh or glab in a terminal, then check again.")))
                .child(action("Check again"))
                .maybe(failed),
            None => muted("No GitHub, GitLab, Azure DevOps, Bitbucket or Codeberg remote for this project."),
        },
        Some(Ok(answer)) if answer.problem.is_some() => {
            let problem = answer.problem.as_ref().unwrap();
            stack().prop("gap", "s").child(muted(problem.message.as_str())).child(action(&problem.action)).maybe(failed)
        }
        Some(Ok(_)) => {
            let shown = filter_pulls(rows, s.filter);
            if shown.is_empty() {
                let more = (s.filter != Filter::All && !rows.is_empty()).then(|| format!("{} open in all.", rows.len()));
                empty(empty_title(s.filter), more.as_deref().unwrap_or_default())
            } else {
                rows_card(shown.iter().map(|r| pull_item(r, s.filter == Filter::All)))
            }
        }
    }
}

/// The chips as `(value, label)`: the drafts one only when there are drafts or it is the chosen filter.
fn chip_options(current: Filter, counts: [usize; 4]) -> Vec<(String, String)> {
    FILTERS
        .iter()
        .enumerate()
        .filter(|(_, (f, _))| *f != Filter::Drafts || counts[3] > 0 || current == Filter::Drafts)
        .map(|(i, (f, label))| (f.name().to_owned(), format!("{label} {}", counts[i])))
        .collect()
}

fn filter_chips(s: &State, counts: [usize; 4]) -> Node {
    let options = chip_options(s.filter, counts);
    let options: Vec<(&str, &str)> = options.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    chips("Show", &options, s.filter.name(), |value| set_filter(filter_of(&json!({ "filter": value })))).segmented()
}

fn build(s: &State) -> Vec<Node> {
    let Some(project) = &s.project else {
        return vec![Node::new("Loading").text("Loading pull requests")];
    };
    let Some((_, name)) = project else {
        return vec![empty("No project", "")];
    };
    let rows: Vec<PullRow> = match &s.answer {
        Some(Ok(PullsAnswer { rows: Some(rows), .. })) => rows.clone(),
        _ => Vec::new(),
    };
    let ready = matches!(&s.answer, Some(Ok(PullsAnswer { rows: Some(_), .. })));
    let counts = count_pulls(&rows);
    let refresh = icon_button("Refresh", &icons::REFRESH_CW, || read(false)).disabled(s.loading);
    let chips_node = ready.then(|| filter_chips(s, counts));
    if s.store == Store::Tab {
        let actions = row().prop("gap", "xs").prop("align", "center").child(refresh);
        let mut top = vec![toolbar_titled(title_with_context("Pull requests", name), Some(actions))];
        top.extend(chips_node.map(|chips| Node::new("Bar").prop("edge", "bottom").child(row().prop("gap", "m").prop("wrap", true).prop("align", "center").child(chips))));
        return vec![page_with(top, [body(s, &rows)])];
    }
    // The Project page's block already has its title bar, so the block has no header of its own: the filters, which carry
    // the counts, and Refresh share one line.
    let controls = row().prop("gap", "s").prop("align", "center").prop("justify", if chips_node.is_some() { "between" } else { "end" }).maybe(chips_node).child(refresh);
    vec![stack().prop("gap", "m").child(controls).child(body(s, &rows))]
}

/// The block's page.
pub fn block_view() -> Vec<Node> {
    STATE.with(|s| build(&s.borrow()))
}

/// The picker's page.
pub fn picker_view() -> Vec<Node> {
    STATE.with(|s| build(&s.borrow()))
}


/// Takes the project on screen from Wings' project event, and says whether it is another one: another project starts
/// over, as its page is its own. A folder of a page tab on screen (`folder`) is within the project and changes nothing.
fn moved_to(project: &Value) -> bool {
    let now = project["path"].as_str().map(|path| (path.to_owned(), project["name"].as_str().unwrap_or_default().to_owned()));
    with(|s| {
        let changed = s.project.as_ref() != Some(&now);
        if changed {
            s.project = Some(now);
            s.answer = None;
        }
        changed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_project_on_screen_starts_the_page_over() {
        let wings = json!({ "id": "/wings", "name": "wings", "path": "/wings" });
        assert!(moved_to(&wings), "the first project heard");
        with(|s| s.answer = Some(Err("kept".into())));
        // A Claude chat tab in a worktree of the same project: the same page.
        assert!(!moved_to(&json!({ "id": "/wings", "name": "wings", "path": "/wings", "folder": "/wings-worktrees/fix" })));
        assert!(with(|s| s.answer.is_some()), "what it shows stays");
        assert!(moved_to(&json!({ "id": "/shop", "name": "shop", "path": "/shop" })), "another project");
        assert!(with(|s| s.answer.is_none()), "what it showed of wings goes");
        assert!(moved_to(&json!(null)), "no project open");
    }

    #[test]
    fn a_status_dot_is_green_unless_its_tone_says_more() {
        assert_eq!(dot_tone("success"), None);
        assert_eq!(dot_tone("warning"), Some("warn"));
        assert_eq!(dot_tone("merged"), Some("info"));
    }

    #[test]
    fn the_drafts_chip_shows_only_when_there_are_drafts_or_it_is_the_chosen_filter() {
        let labels = |current, counts| chip_options(current, counts).into_iter().map(|(_, label)| label).collect::<Vec<_>>();
        assert_eq!(labels(Filter::All, [3, 1, 0, 0]), ["All open 3", "Mine 1", "Review requested 0"]);
        assert_eq!(labels(Filter::All, [3, 1, 0, 2]).last().map(String::as_str), Some("Drafts 2"));
        assert_eq!(labels(Filter::Drafts, [3, 1, 0, 0]).last().map(String::as_str), Some("Drafts 0"));
    }

    fn row_json(id: &str, number: u32) -> Value {
        json!({
            "id": id, "title": "[WEB-4] Add retry", "number": format!("#{number}"), "author": "ada", "branch": "retry → main", "scope": "mine", "draft": false,
            "badge": { "text": "Checks failing", "tone": "danger" }, "review": "Changes requested", "checks": { "failed": 1, "running": 0, "queued": 0, "passed": 3, "skipped": 0 },
            "updated": "2026-11-01T09:00:00Z", "unread": 0,
            "args": { "provider": "github", "url": "https://github.com/acme/web/pull/1", "number": number, "repo": "acme/web", "title": "Add retry", "cwd": "/w" },
        })
    }

    fn state(store: Store) -> State {
        let rows: Vec<PullRow> = vec![serde_json::from_value(row_json("a", 1)).unwrap(), serde_json::from_value(row_json("b", 2)).unwrap()];
        State {
            store,
            project: Some(Some(("/w".into(), "web".into()))),
            answer: Some(Ok(PullsAnswer { rows: Some(rows), ..PullsAnswer::default() })),
            filter: Filter::All,
            loading: false,
            signing: false,
            sign_error: None,
            reads: 0,
            on_open: None,
        }
    }

    fn json_of(nodes: Vec<Node>) -> String {
        nodes.into_iter().map(|n| wings_plugin::ui::tree_json(n).to_string()).collect()
    }

    /// The design system's list: the filters are a segmented track in a bar under the header, Refresh is a ghost icon
    /// button with a tooltip, and the pull requests are rows of one card, not a card each.
    #[test]
    fn the_tab_has_a_segmented_filter_bar_an_icon_refresh_and_its_rows_in_one_card() {
        let page = json_of(build(&state(Store::Tab)));
        assert!(page.contains(r#""segmented":true"#) && page.contains(r#""type":"Bar""#), "{page}");
        assert!(page.contains(r#""label":"Refresh""#) && page.contains(r#""size":"icon-sm""#) && !page.contains(r#""children":["Refresh"]"#), "{page}");
        assert_eq!(page.matches(r#""type":"Card""#).count(), 1, "{page}");
        assert_eq!(page.matches(r#""variant":"rows""#).count(), 1, "{page}");
        assert_eq!(page.matches(r#""type":"ListButton""#).count(), 2, "{page}");
    }

    /// The facts under a row's title are split by " · " and muted: the status is one filled badge, not coloured words.
    #[test]
    fn a_rows_status_is_a_filled_badge_and_its_facts_are_plain_muted_text() {
        let row = json_of(vec![pull_item(&serde_json::from_value(row_json("a", 1)).unwrap(), true)]);
        assert_eq!(row.matches(r#""text":"·""#).count(), 4, "{row}");
        assert_eq!(row.matches(r#""type":"Badge""#).count(), 3, "{row}");
        assert!(row.contains(r#""tone":"danger""#) && !row.contains(r#""tone":"success""#), "{row}");
        assert!(!row.contains(r#""type":"Text","props":{"size":"small","tone":"danger""#), "{row}");
    }

    #[test]
    fn the_block_has_the_track_and_the_icon_refresh_too() {
        let block = json_of(build(&state(Store::Block)));
        assert!(block.contains(r#""segmented":true"#) && block.contains(r#""label":"Refresh""#), "{block}");
    }

    /// The chips already say how many are open, so the block has no muted "4 open" imitating a header, and Refresh sits
    /// in the same row as the chips.
    #[test]
    fn the_block_has_no_count_header_and_refresh_shares_the_filter_row() {
        let block = json_of(build(&state(Store::Block)));
        assert!(!block.contains(r#"" open""#) && !block.contains(r#""4 open""#) && !block.contains(r#""2 open""#), "{block}");
        let tree: Value = serde_json::from_str(&block).unwrap();
        let controls = tree["children"][0].to_string();
        assert!(controls.contains(r#""type":"Chips""#) && controls.contains(r#""label":"Refresh""#), "{tree}");
    }
}
