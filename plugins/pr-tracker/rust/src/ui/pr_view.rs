//! The PR tab's page, drawn from what the page holds (`pr_page.rs` owns the loading and the requests). What only the
//! drawing needs to remember, like the tab picked, which thread is folded and the text of a reply, is kept here.

use crate::{
    pr_page::{self, Page, Snip},
    providers::{info, ref_of},
    pushes::{Entry, day_time, timeline_of},
    snippet::snippet_span,
    threads::{Kind, Thread, focus_of, is_automated, kind_of, one_line, threads_of},
    types::{Comment, Overview, Push, Reviewer, SnippetLine, Status},
    ui::{checks_view, markdown::markdown, parts, run_view},
    vote::{AZURE_VOTES, reviewable},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, card, confirm, empty, heading, muted, notice, page, row, stack, text},
};

/// The native app draws a page of at most 5,000 nodes, so a long conversation shows in steps.
const STEP: usize = 30;
/// How many commits a push lists before "Show all".
const COMMITS_SHOWN: usize = 5;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Overview,
    Checks,
    Files,
}

/// What the user picked and has yet to confirm: Approve, or one of Azure DevOps' votes.
#[derive(Clone)]
struct Pending {
    label: String,
    vote: Option<String>,
}

#[derive(Default)]
struct ReplyBox {
    open: bool,
    text: String,
    sending: bool,
    error: Option<String>,
}

#[derive(Default)]
struct Ui {
    tab: Tab,
    pending: Option<Pending>,
    approving: bool,
    verdict: Option<(bool, String)>,
    show_resolved: bool,
    shown: usize,
    /// Threads whose fold is the other way from how they start.
    toggled: HashSet<i64>,
    replies: HashMap<i64, ReplyBox>,
    all_commits: HashSet<i64>,
    more_text: HashSet<i64>,
    copied: bool,
    vote_menu: bool,
    /// Threads that show the latest code instead of the code as commented.
    latest: HashSet<i64>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn ui<T>(change: impl FnOnce(&mut Ui) -> T) -> T {
    UI.with(|ui| change(&mut ui.borrow_mut()))
}

/// Forgets what the drawing remembered, for another pull request.
pub fn reset() {
    UI.with(|ui| *ui.borrow_mut() = Ui::default());
}

/// Whether a thread's code shows the latest push's, for the read that fetches it.
pub fn latest_code(id: i64) -> bool {
    ui(|u| u.latest.contains(&id))
}

fn shown_limit(u: &Ui) -> usize {
    if u.shown == 0 { STEP } else { u.shown }
}

/// Whether a thread starts open: a resolved one is folded unless the list says to expand them.
fn starts_open(kind: Kind, show_resolved: bool) -> bool {
    kind != Kind::Resolved || show_resolved
}

fn is_open(u: &Ui, kind: Kind, id: i64) -> bool {
    starts_open(kind, u.show_resolved) != u.toggled.contains(&id)
}

/// The code threads whose card shows now and that carry no code of their own: what `pr_page` reads lines for.
pub fn wanted_snippets(p: &Page) -> Vec<(Comment, bool)> {
    let Some(comments) = &p.comments else { return Vec::new() };
    ui(|u| {
        timeline_of(&threads_of(comments), &p.pushes)
            .into_iter()
            .take(shown_limit(u))
            .filter_map(|e| match e {
                Entry::Thread { thread, .. } if is_open(u, kind_of(&thread), thread.root.id) => Some(thread.root),
                _ => None,
            })
            .filter(|c| c.path.is_some() && c.range.is_some() && c.snippet.is_none())
            .map(|c| {
                let now = u.latest.contains(&c.id);
                (c, now)
            })
            .collect()
    })
}

// ---------- small pieces ----------

fn col(gap: &str) -> Node {
    stack().prop("gap", gap)
}

fn line(gap: &str) -> Node {
    row().prop("gap", gap)
}

fn button(label: impl Into<String>, variant: &str, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").prop("variant", variant).prop("size", "sm").on("onClick", move |_| on_click()).text(label)
}

fn busy_button(label: impl Into<String>, variant: &str, busy: bool, on_click: impl Fn() + 'static) -> Node {
    let node = Node::new("Button").prop("variant", variant).prop("size", "sm").on("onClick", move |_| on_click());
    if busy { node.prop("disabled", true).child(wings_plugin::ui::spinner()).text(label) } else { node.text(label) }
}

fn avatar(name: &str, small: bool) -> Node {
    let node = Node::new("Avatar").prop("name", name);
    if small { node.prop("size", "small") } else { node }
}

fn skeleton(width: Value, height: u32) -> Node {
    Node::new("Skeleton").prop("width", width).prop("height", height)
}

fn small(text_: impl Into<String>) -> Node {
    text(text_).prop("size", "small")
}

fn tone_badge(label: &str, tone: &str) -> Node {
    match tone {
        "neutral" => Node::new("Badge").text(label),
        "warning" => Node::new("Badge").prop("tone", "warn").text(label),
        "merged" => Node::new("Badge").prop("tone", "info").text(label),
        tone => Node::new("Badge").prop("tone", tone).text(label),
    }
}

fn outline(label: impl Into<String>, tone: Option<&str>) -> Node {
    let badge = Node::new("Badge").prop("variant", "outline");
    match tone {
        Some(tone) => badge.prop("tone", tone),
        None => badge,
    }
    .text(label)
}

/// "opened 3 days ago".
fn when(prefix: &str, iso: &str) -> Option<String> {
    parts::time_text(iso).map(|ago| format!("{prefix}{ago}"))
}

fn comment_head(author: &str, iso: Option<&str>, suffix: &str, is_author: bool, bot: bool, end: Vec<Node>) -> Node {
    let when = iso.and_then(|iso| when(suffix, iso)).or_else(|| (!suffix.is_empty()).then(|| suffix.trim_end().to_owned()));
    line("s")
        .prop("wrap", true)
        .child(avatar(author, false))
        .child(text(author).prop("weight", "strong"))
        .maybe(is_author.then(|| parts::tag("Author", false)))
        .maybe(bot.then(|| parts::tag("Bot", false)))
        .maybe(when.map(|when| small(when).prop("tone", "muted")))
        .maybe((!end.is_empty()).then(|| line("s").prop("grow", true).prop("justify", "end").children(end)))
}

// ---------- the page ----------

pub fn build(p: &Page) -> Vec<Node> {
    let Some(args) = &p.args else { return Vec::new() };
    let st = p.overview.as_ref().map(|o| &o.status);
    let reference = format!("{} {}", args.repo, ref_of(args.provider, args.number));
    let (tab, pending, approving, verdict, vote_menu) = ui(|u| (u.tab, u.pending.clone(), u.approving, u.verdict.clone(), u.vote_menu));
    let run_open = tab == Tab::Checks && st.is_some_and(run_view::is_open);
    let open = reviewable(st);

    let mut actions = vec![
        button("Refresh", "outline", pr_page::refresh),
        busy_button("Send to agent", "outline", p.busy == Some("agent"), || pr_page::run("agent", None)),
        {
            let url = args.url.clone();
            button(format!("Open in {}", info(args.provider).name), "outline", move || host::open_url(&url))
        },
    ];
    if p.can.approve && open {
        actions.push(busy_button("Approve", "outline", approving, || ui(|u| u.pending = Some(Pending { label: "Approve".into(), vote: None }))));
    }
    if p.can.vote && open {
        let menu = Node::new("Button").prop("variant", "outline").prop("size", "sm").prop("disabled", approving).on("onClick", |_| ui(|u| u.vote_menu = !u.vote_menu)).text("Vote ▾");
        actions.push(menu);
    }
    actions.push(busy_button("Open diff", "default", p.busy == Some("diff"), || pr_page::run("diff", None)));
    let mut toolbar = Node::new("Toolbar");
    toolbar = match &p.back {
        Some(back) => {
            let back = back.clone();
            toolbar.child(Node::new("Slot").prop("name", "children").child(
                line("s").child(button("← All pull requests", "ghost", move || back())).child(muted("/")).child(muted(reference.clone()).prop("truncate", true)),
            ))
        }
        None => toolbar.child(Node::new("Slot").prop("name", "title").text(reference.clone())),
    };
    toolbar = toolbar.child(Node::new("Slot").prop("name", "actions").children(actions));

    let mut body: Vec<Node> = Vec::new();
    if let Some(pending) = pending {
        let (label, vote) = (pending.label.clone(), pending.vote.clone());
        let sha = st.map(|s| s.key.clone()).unwrap_or_default();
        let send = busy_button(label.clone(), "default", approving, move || confirm_pending(vote.clone(), label.clone(), sha.clone()));
        let cancel = button("Cancel", "outline", || ui(|u| u.pending = None));
        body.push(confirm(format!("{} on {reference} as you?", pending.label), [text("Everyone on it sees your vote.")], [send, cancel]));
    }
    if vote_menu {
        body.push(
            line("s").prop("wrap", true).children(AZURE_VOTES.iter().map(|(id, label)| {
                let (key, id, label) = (id.to_string(), id.to_string(), label.to_string());
                button(label.clone(), "ghost", move || ui(|u| {
                    u.vote_menu = false;
                    u.pending = Some(Pending { label: label.clone(), vote: Some(id.clone()) });
                }))
                .key(key)
            })),
        );
    }
    body.push(wrap(p, st, run_open, verdict));
    vec![page(toolbar, body)]
}

fn confirm_pending(vote: Option<String>, label: String, sha: String) {
    ui(|u| {
        u.approving = true;
        u.verdict = None;
    });
    spawn(async move {
        let result = match vote {
            Some(vote) => pr_page::vote(vote).await,
            None => pr_page::approve(sha).await,
        };
        ui(|u| {
            u.approving = false;
            match result {
                Ok(()) => {
                    u.verdict = Some((true, format!("{label}: done.")));
                    u.pending = None;
                }
                Err(error) => u.verdict = Some((false, error)),
            }
        });
    });
}

fn wrap(p: &Page, st: Option<&Status>, run_open: bool, verdict: Option<(bool, String)>) -> Node {
    let args = p.args.as_ref().unwrap();
    let title = st.map_or(args.title.as_str(), |s| s.title.as_str());
    let number = ref_of(args.provider, args.number);
    let overview = p.overview.as_ref();
    let header = col("m")
        .child(line("m").prop("align", "baseline").child(parts::large_text(title).prop("weight", "strong")).child(parts::large_text(number).prop("tone", "muted")))
        .child(
            line("l")
                .prop("wrap", true)
                .maybe(st.map(|s| tone_badge(&s.label, &s.tone)))
                .maybe(st.filter(|s| !s.branch.is_empty()).map(|s| branch(&s.branch)))
                .maybe(overview.map(|o| {
                    line("s")
                        .child(avatar(&o.author, true))
                        .child(small(o.author.as_str()).prop("weight", "strong"))
                        .maybe(o.created_at.as_deref().and_then(|iso| when("opened ", iso)).map(|t| small(t).prop("tone", "muted")))
                }))
                .maybe(st.filter(|s| !s.updated.is_empty()).and_then(|s| when("updated ", &s.updated)).map(|t| small(t).prop("tone", "muted"))),
        )
        .maybe(p.notice.as_ref().map(|n| notice("danger", n.as_str())))
        .maybe(verdict.map(|(ok, t)| notice(if ok { "success" } else { "danger" }, t)));

    let content = match (&p.problem, p.error(), overview, st) {
        (Some(problem), _, None, _) => {
            let action = problem.action.clone();
            let click = busy_button(action, "default", p.problem_busy, pr_page::fix_problem);
            vec![parts_card(vec![text("Couldn't load this pull request").prop("weight", "strong"), muted(problem.message.as_str()), row().child(click)])]
        }
        (_, Some(error), None, _) => vec![parts_card(vec![
            text("Couldn't load this pull request").prop("weight", "strong"),
            text(error).prop("tone", "danger"),
            row().child(button("Try again", "outline", pr_page::refresh)),
        ])],
        (_, _, Some(overview), Some(st)) => loaded(p, overview, st),
        _ => vec![
            col("s").child(Node::new("Spinner").prop("label", "Loading pull request")).children([skeleton(json!("40%"), 14), skeleton(Value::Null, 90), skeleton(Value::Null, 120), skeleton(Value::Null, 70)]),
        ],
    };
    col("l").prop("maxWidth", if run_open { 1100 } else { 860 }).child(header).children(content)
}

fn parts_card(content: Vec<Node>) -> Node {
    card().child(col("m").children(content))
}

fn branch(branch: &str) -> Node {
    let (source, target) = match branch.split_once(" → ") {
        Some((s, t)) => (s.to_owned(), Some(t.to_owned())),
        None => (branch.to_owned(), None),
    };
    let copied = ui(|u| u.copied);
    let name = source.clone();
    let copy = Node::new("Button")
        .prop("variant", "ghost")
        .prop("size", "icon-xs")
        .prop("label", if copied { "Copied".to_owned() } else { format!("Copy branch name {source}") })
        .on("onClick", move |_| {
            drop(host::call("copyText", json!({ "text": name })));
            ui(|u| u.copied = true);
            spawn(async {
                host::sleep(1500).await;
                ui(|u| u.copied = false);
            });
        })
        .text(if copied { "✓" } else { "⧉" });
    line("s")
        .child(text(source).prop("mono", true).prop("size", "small").prop("truncate", true))
        .maybe(target.map(|t| line("s").child(muted("→")).child(text(t).prop("mono", true).prop("size", "small").prop("truncate", true))))
        .child(copy)
}

const VOTE_WORD: [(&str, &str); 4] = [("approved", "approved"), ("changes", "requested changes"), ("waiting", "is waiting for the author"), ("none", "has not voted")];

fn vote_word(vote: &str) -> &'static str {
    VOTE_WORD.iter().find(|(v, _)| *v == vote).map_or("has not voted", |(_, w)| w)
}

/// A disc with a check, or an exclamation mark, cut out of it.
fn vote_mark(vote: &str, label: String) -> Node {
    let shape = if vote == "approved" { "M3.1 4.7 4.3 5.95 6.88 2.94 7.92 3.86 4.3 7.85 2.1 5.7Z" } else { "M4.3 2h1.4v4.2H4.3ZM4.3 7.1h1.4v1.4H4.3Z" };
    Node::new("Svg")
        .prop("viewBox", "0 0 10 10")
        .prop("size", 10)
        .prop("tone", if vote == "approved" { "success" } else { "danger" })
        .prop("label", label)
        .prop("shapes", json!([["path", { "d": format!("M5 0a5 5 0 1 1 0 10A5 5 0 0 1 5 0Z{shape}"), "fill": "currentColor", "fillRule": "evenodd" }]]))
}

fn review_chip(overview: &Overview) -> Node {
    let (status, reviewers) = (&overview.status, overview.reviewers.as_deref().unwrap_or_default());
    let review = status.review.to_lowercase();
    let tone = if reviewers.iter().any(|r| r.vote == "changes" || r.vote == "waiting") || review.contains("changes") || review.contains("rejected") || review.contains("waiting for author") {
        Some("danger")
    } else if review.contains("approved") {
        Some("success")
    } else {
        None
    };
    outline(status.review.as_str(), tone).maybe((!reviewers.is_empty()).then(|| {
        line("s").children(reviewers.iter().map(|r: &Reviewer| {
            let label = format!("{} {}", r.name, vote_word(&r.vote));
            line("none").key(r.name.clone()).child(avatar(&r.name, true)).maybe((r.vote != "none").then(|| vote_mark(&r.vote, label)))
        }))
    }))
}

fn checks_chip(st: &Status) -> Node {
    let c = st.checks;
    let parts: Vec<String> = [(c.failed, "failed"), (c.running, "running"), (c.passed, "passed")].into_iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}")).collect();
    let tone = if c.failed > 0 { "danger" } else if c.running > 0 { "warn" } else { "success" };
    outline(format!("{} ›", parts.join(" · ")), Some(tone)).on("onClick", |_| ui(|u| u.tab = Tab::Checks))
}

fn loaded(p: &Page, overview: &Overview, st: &Status) -> Vec<Node> {
    let total = st.checks.passed + st.checks.failed + st.checks.running;
    let comments = p.comments.as_deref();
    let people = comments.map_or(0, <[Comment]>::len);
    let unresolved = comments.map(|cs| cs.iter().filter(|c| c.reply_to.is_none() && !c.system && c.thread.as_ref().is_some_and(|t| t.state == "active")).count());
    let tab = ui(|u| u.tab);
    let summary = line("m")
        .prop("wrap", true)
        .child(review_chip(overview).key("review"))
        .child(if total > 0 { checks_chip(st) } else { outline("No checks", None) }.key("checks"))
        .maybe((st.label == "Conflicts").then(|| outline("Conflicts", Some("danger"))))
        .maybe(st.merge.as_ref().map(|m| outline(m.as_str(), None)))
        .maybe(st.changes.as_ref().map(|c| outline(c.as_str(), None)));
    let value = |t: Tab| match t {
        Tab::Overview => "overview",
        Tab::Checks => "checks",
        Tab::Files => "files",
    };
    let tabs = Node::new("Chips")
        .prop("label", "Pull request")
        .prop("line", true)
        .prop("value", value(tab))
        .prop(
            "options",
            json!([
                { "value": "overview", "label": format!("Overview{}", if people > 0 { format!(" {people}") } else { String::new() }) },
                { "value": "checks", "label": format!("Checks{}", if total > 0 { format!(" {}/{total}", st.checks.passed) } else { String::new() }) },
                { "value": "files", "label": "Files" },
            ]),
        )
        .on("onChange", move |event| {
            let next = match event["value"].as_str() {
                Some("checks") => Tab::Checks,
                Some("files") => Tab::Files,
                _ => Tab::Overview,
            };
            if next != tab {
                // The Checks tab comes back to its list.
                run_view::close();
            }
            ui(|u| u.tab = next);
        });
    let panel = match tab {
        Tab::Overview => overview_tab(p, overview),
        Tab::Checks => parts_card(vec![checks_view::build(st, unresolved)]),
        Tab::Files => parts_card(vec![
            text(st.changes.clone().unwrap_or_else(|| "Changed files".to_owned())).prop("weight", "strong"),
            muted("The changed files open in Wings' diff viewer, with the review comments on their lines."),
            row().child(busy_button("Open diff", "default", p.busy == Some("diff"), || pr_page::run("diff", None))),
        ]),
    };
    vec![summary].into_iter().chain([wrap_tabs(tabs, panel)]).chain(p.error().map(|e| notice("danger", e))).collect()
}

fn wrap_tabs(tabs: Node, panel: Node) -> Node {
    col("m").child(tabs).child(panel)
}

// ---------- overview: description and conversation ----------

fn overview_tab(p: &Page, overview: &Overview) -> Node {
    let (show_resolved, limit) = ui(|u| (u.show_resolved, shown_limit(u)));
    let entries = p.comments.as_ref().map(|cs| timeline_of(&threads_of(cs), &p.pushes));
    let resolved = entries.as_ref().map_or(0, |es| es.iter().filter(|e| matches!(e, Entry::Thread { thread, .. } if kind_of(thread) == Kind::Resolved)).count());
    let threads = entries.as_ref().map_or(0, |es| es.iter().filter(|e| matches!(e, Entry::Thread { .. })).count());
    let latest = p.pushes.iter().map(|push| push.id).max();

    let description = card().child(
        col("m")
            .child(comment_head(&overview.author, overview.created_at.as_deref(), "opened this ", true, false, Vec::new()))
            .child(if overview.body.trim().is_empty() { muted("No description provided.") } else { markdown(&overview.body) }),
    );
    let toggle = (resolved > 0).then(|| {
        Node::new("Button")
            .prop("chip", true)
            .prop("pressed", show_resolved)
            .on("onClick", |_| {
                ui(|u| {
                    u.show_resolved = !u.show_resolved;
                    u.toggled.clear();
                });
                pr_page::ensure_snippets();
            })
            .text(format!("{} {resolved} resolved", if show_resolved { "Collapse" } else { "Expand" }))
    });
    let head = line("m")
        .child(heading(3, format!("Conversation{}", if entries.is_some() { format!(" · {threads}") } else { String::new() })).prop("grow", true))
        .maybe(toggle);

    let conversation = match &entries {
        None if p.error().is_some() => None,
        None => Some(col("s").child(Node::new("Spinner").prop("label", "Loading conversation")).children([skeleton(Value::Null, 80), skeleton(Value::Null, 80)])),
        Some(es) if es.is_empty() => Some(empty("No comments yet", "Review threads and comments on the pull request show up here.")),
        Some(es) => Some(col("m").children(es.iter().take(limit).map(|e| match e {
            Entry::Push { push, .. } => push_card(push),
            Entry::Thread { thread, .. } => thread_card(p, thread, &overview.author, latest),
        }))),
    };
    let more = entries.as_ref().filter(|es| es.len() > limit).map(|es| {
        let rest = es.len() - limit;
        row().child(
            Node::new("Button").prop("variant", "outline").on("onClick", |_| {
                ui(|u| u.shown = shown_limit(u) + STEP);
                pr_page::ensure_snippets();
            }).text(format!("Show {} more of {rest} not shown", rest.min(STEP))),
        )
    });
    col("l").child(description).child(head).maybe(conversation).maybe(more)
}

fn commit_line(c: &crate::types::PushCommit) -> Vec<Node> {
    vec![
        text(if c.message.is_empty() { "No message" } else { c.message.as_str() }).prop("truncate", true).prop("grow", true).prop("size", "small"),
        line("s")
            .child(text(c.sha.chars().take(7).collect::<String>()).prop("mono", true).prop("size", "tiny"))
            .child(muted(c.author.as_str()).prop("size", "tiny"))
            .child(muted(day_time(&c.at, host::now())).prop("size", "tiny")),
    ]
}

/// A push: one quiet line, then its commits in muted text.
fn push_card(push: &Push) -> Node {
    let id = push.id;
    let all = ui(|u| u.all_commits.contains(&id));
    let count = format!("{}{}", push.commits.len(), if push.more { "+" } else { "" });
    let did = format!(
        "{} {count} {}",
        if push.created { "opened the pull request with" } else { "pushed" },
        if push.commits.len() == 1 && !push.more { "commit" } else { "commits" }
    );
    let commits = push.commits.iter().take(if all { usize::MAX } else { COMMITS_SHOWN }).map(|c| {
        let (sha, message) = (c.sha.clone(), c.message.clone());
        Node::new("ListButton")
            .key(c.sha.clone())
            .prop("title", format!("See the changes in {}: {}", c.sha.chars().take(7).collect::<String>(), if c.message.is_empty() { "No message" } else { &c.message }))
            .on("onClick", move |_| pr_page::open_commit(sha.clone(), message.clone()))
            .children(commit_line(c))
    });
    line("m")
        .prop("align", "start")
        .key(format!("push-{id}"))
        .child(Node::new("Badge").prop("variant", "outline").prop("size", "sm").child(text(id.to_string()).prop("mono", true).prop("tone", "muted")))
        .child(
            col("s")
                .prop("grow", true)
                .child(
                    line("s")
                        .prop("wrap", true)
                        .child(avatar(&push.author, true))
                        .child(small(push.author.as_str()).prop("weight", "strong"))
                        .child(small(did).prop("tone", "muted"))
                        .maybe(when("", &push.at).map(|t| small(t).prop("tone", "muted"))),
                )
                .child(col("xs").children(commits))
                .maybe((push.commits.len() > COMMITS_SHOWN).then(|| {
                    row().child(
                        Node::new("Button")
                            .prop("variant", "ghost")
                            .prop("size", "sm")
                            .prop("aria-expanded", all)
                            .on("onClick", move |_| ui(|u| {
                                if !u.all_commits.remove(&id) {
                                    u.all_commits.insert(id);
                                }
                            }))
                            .text(if all { "Show fewer commits".to_owned() } else { format!("Show all {count} commits") }),
                    )
                })),
        )
}

fn status_badge(root: &Comment) -> Option<Node> {
    root.thread.as_ref().map(|t| Node::new("Badge").prop("variant", "outline").prop("size", "sm").prop("tone", if t.state == "active" { "warn" } else { "success" }).text(t.label.as_str()))
}

fn toggle_thread(id: i64) {
    ui(|u| {
        if !u.toggled.remove(&id) {
            u.toggled.insert(id);
        }
    });
    pr_page::ensure_snippets();
}

fn thread_card(p: &Page, thread: &Thread, pr_author: &str, latest: Option<i64>) -> Node {
    let (root, replies) = (&thread.root, &thread.replies);
    let kind = kind_of(thread);
    let id = root.id;
    let open = ui(|u| is_open(u, kind, id));
    let where_ = root.path.as_ref().map(|path| match root.line {
        Some(line) if line > 0 => format!("{path}:{line}"),
        _ => path.clone(),
    });
    if !open {
        return Node::new("ListButton")
            .key(format!("thread-{id}"))
            .prop("aria-expanded", false)
            .on("onClick", move |_| toggle_thread(id))
            .child(avatar(&root.author, true))
            .child(text(root.author.as_str()).prop("weight", "strong"))
            .child(text(match one_line(&root.body, 140) {
                t if t.is_empty() => "No text".to_owned(),
                t => t,
            }).prop("truncate", true).prop("grow", true))
            .maybe(where_.as_ref().map(|w| text(w.as_str()).prop("mono", true).prop("size", "tiny").prop("truncate", true)))
            .maybe((!replies.is_empty()).then(|| muted(format!("{} {}", replies.len(), if replies.len() == 1 { "reply" } else { "replies" })).prop("size", "tiny")))
            .maybe(status_badge(root));
    }
    let file = root.path.as_ref().map(|path| {
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        let focus = focus_of(root);
        col("m")
            .child(
                line("m")
                    .prop("justify", "between")
                    .child(
                        col("xs")
                            .prop("grow", true)
                            .child(text(name).prop("weight", "strong").prop("truncate", true))
                            .child(muted(format!("{path}{}", root.line.map_or(String::new(), |l| format!(":{l}")))).prop("mono", true).prop("size", "small").prop("truncate", true)),
                    )
                    .child(button("View in diff", "ghost", move || pr_page::run("diff", focus.clone()))),
            )
            .maybe(snippet(p, root, latest))
    });
    let mut end: Vec<Node> = status_badge(root).into_iter().collect();
    if kind == Kind::Resolved {
        end.push(Node::new("Button").prop("variant", "ghost").prop("size", "sm").prop("aria-expanded", true).on("onClick", move |_| toggle_thread(id)).text("Collapse"));
    }
    let automated = kind == Kind::Automated;
    let reply_box = (root.can_reply == Some(true) && p.overview.is_some()).then(|| reply_box(root));
    card().key(format!("thread-{id}")).child(
        col("m")
            .maybe(file)
            .child(comment_head(&root.author, Some(&root.created_at), "", root.author == pr_author, is_automated(root), end))
            .child(body(id, &root.body, automated))
            .maybe((!replies.is_empty()).then(|| {
                col("m").children(replies.iter().map(|r| {
                    col("s")
                        .key(format!("reply-{}", r.id))
                        .child(comment_head(&r.author, Some(&r.created_at), "", r.author == pr_author, is_automated(r), Vec::new()))
                        .child(body(r.id, &r.body, automated))
                }))
            }))
            .maybe(reply_box),
    )
}

/// A comment's text; a long bot comment shows its first lines until asked for the rest.
fn body(id: i64, source: &str, clamp: bool) -> Node {
    let long = clamp && (source.chars().count() > 1600 || source.lines().count() > 24);
    let more = ui(|u| u.more_text.contains(&id));
    let shown: String = if long && !more { source.lines().take(24).collect::<Vec<_>>().join("\n").chars().take(1600).collect() } else { source.to_owned() };
    col("s").child(markdown(&shown)).maybe(long.then(|| {
        row().child(
            Node::new("Button")
                .prop("variant", "ghost")
                .prop("size", "sm")
                .prop("aria-expanded", more)
                .on("onClick", move |_| ui(|u| {
                    if !u.more_text.remove(&id) {
                        u.more_text.insert(id);
                    }
                }))
                .text(if more { "Show less" } else { "Show more" }),
        )
    }))
}

// ---------- the code a thread is on ----------

/// Blank lines numbered like the snippet that will replace them, so the card doesn't jump when the code arrives.
fn placeholder_lines(start: i64, end: i64, below: bool) -> Vec<SnippetLine> {
    let span = snippet_span(start, end);
    (span.from..=(if below { span.to } else { span.last })).map(|n| SnippetLine { n, text: String::new(), hit: n >= start && n <= span.last, added: None, removed: None }).collect()
}

fn code_lines(lines: &[SnippetLine], busy: bool) -> Node {
    let rows = lines.iter().map(|l| {
        let tone = if l.added == Some(true) { Some("success") } else if l.removed == Some(true) { Some("danger") } else { None };
        let mut code = text(if l.text.is_empty() { " " } else { l.text.as_str() }).prop("mono", true).prop("size", "small").prop("grow", true);
        if let Some(tone) = tone {
            code = code.prop("tone", tone);
        }
        if l.hit {
            code = code.prop("weight", "strong");
        }
        line("m").prop("align", "baseline").key(l.n.to_string()).child(muted(l.n.to_string()).prop("mono", true).prop("size", "tiny")).child(code)
    });
    card().child(col("none").maybe(busy.then(|| Node::new("Spinner").prop("label", "Loading the code"))).children(rows))
}

fn snippet(p: &Page, root: &Comment, latest: Option<i64>) -> Option<Node> {
    let id = root.id;
    let now = ui(|u| u.latest.contains(&id));
    let held = p.snippets.get(&(id, now));
    let read = match held {
        Some(Snip::Done(lines)) => lines.clone(),
        _ => None,
    };
    let lines = root.snippet.clone().or(read);
    let gone = root.snippet.is_none() && now && matches!(held, Some(Snip::Done(None)));
    let iteration = root.range.as_ref().and_then(|r| r.iteration);
    let older = iteration.zip(latest).is_some_and(|(i, l)| i < l);
    let loading = lines.is_none() && p.can.snippet && root.path.is_some() && root.range.is_some() && !gone;
    let shown = lines.clone().or_else(|| loading.then(|| root.range.as_ref().map(|r| placeholder_lines(r.start, r.end, iteration.is_some()))).flatten());
    let bar = older.then(|| {
        let (it, l) = (iteration.unwrap(), latest.unwrap());
        line("s")
            .prop("justify", "between")
            .child(muted(if now { format!("Latest code, push {l}") } else { format!("As of push {it}") }).prop("size", "tiny"))
            .child(button(if now { format!("Show push {it}") } else { "Show latest".to_owned() }, "ghost", move || {
                ui(|u| {
                    if !u.latest.remove(&id) {
                        u.latest.insert(id);
                    }
                });
                pr_page::ensure_snippets();
            }))
    });
    if gone {
        return Some(col("s").maybe(bar).child(small("These lines are no longer in the latest code.").prop("tone", "muted")));
    }
    let shown = shown?;
    Some(col("s").maybe(bar).child(code_lines(&shown, lines.is_none())))
}

// ---------- replying ----------

fn reply_box(root: &Comment) -> Node {
    let id = root.id;
    let (open, text_, sending, error) = ui(|u| {
        let b = u.replies.entry(id).or_default();
        (b.open, b.text.clone(), b.sending, b.error.clone())
    });
    if !open {
        return row().child(button("Reply", "outline", move || ui(|u| u.replies.entry(id).or_default().open = true)));
    }
    let root = root.clone();
    let field = Node::new("Textarea")
        .prop("label", "Reply")
        .prop("rows", 3)
        .prop("value", text_.as_str())
        .prop("autoFocus", true)
        .on("onChange", move |event| ui(|u| u.replies.entry(id).or_default().text = event["value"].as_str().unwrap_or_default().to_owned()));
    let send = busy_button("Reply", "default", sending, move || {
        let body = ui(|u| u.replies.entry(id).or_default().text.trim().to_owned());
        if body.is_empty() {
            return;
        }
        ui(|u| {
            let b = u.replies.entry(id).or_default();
            b.sending = true;
            b.error = None;
        });
        let root = root.clone();
        spawn(async move {
            let result = pr_page::reply(root, body).await;
            ui(|u| {
                let b = u.replies.entry(id).or_default();
                b.sending = false;
                match result {
                    Ok(()) => {
                        b.text.clear();
                        b.open = false;
                    }
                    Err(error) => b.error = Some(error),
                }
            });
        });
    });
    col("s").child(field).maybe(error.map(|e| notice("danger", e))).child(line("s").child(send).child(button("Cancel", "ghost", move || ui(|u| u.replies.entry(id).or_default().open = false))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resolved_thread_starts_folded_and_the_expand_button_or_a_click_flips_it() {
        let mut u = Ui::default();
        assert!(is_open(&u, Kind::Open, 1) && is_open(&u, Kind::Automated, 1) && !is_open(&u, Kind::Resolved, 1));
        u.toggled.insert(1);
        assert!(!is_open(&u, Kind::Open, 1) && is_open(&u, Kind::Resolved, 1));
        u.show_resolved = true;
        u.toggled.clear();
        assert!(is_open(&u, Kind::Resolved, 1));
    }

    #[test]
    fn a_snippets_placeholder_has_the_lines_the_code_will() {
        let lines = placeholder_lines(10, 10, true);
        assert_eq!((lines.first().map(|l| l.n), lines.last().map(|l| l.n)), (Some(6), Some(13)));
        assert_eq!(lines.iter().filter(|l| l.hit).count(), 1);
        assert_eq!(placeholder_lines(10, 10, false).last().map(|l| l.n), Some(10));
    }

    #[test]
    fn a_vote_is_worded_as_the_desktop_words_it() {
        assert_eq!(vote_word("changes"), "requested changes");
        assert_eq!(vote_word("nothing"), "has not voted");
    }
}
