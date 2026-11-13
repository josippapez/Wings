//! The work item tab: loads the work item and its parts, and draws them. The page shows at once and each part fills in
//! as its call returns. It runs `az` itself, and tells the main script when it moved a work item, so the pane's badge
//! shows the new state.

use crate::{
    az::{az, az_run, invoke, query, work_item},
    html::{Block, blocks_of},
    item::{
        Activity, Change, Comment, ItemArgs, LinkIds, LinkedItem, PrLink, Revision, all_link_ids, comments_of, discussion_of, ids_query, item_args, item_tab_args, js_number, last_segment, link_ids, linked_items,
        pr_status, pull_request_links, revisions_of, summary_of, tags_of,
    },
    parts::{avatar, badge, card_of, gap, large_title, pill, pill_button, skeleton, skeletons, strong, time_text, tone_dot, type_dot},
    sections::state_tone,
    type_color::type_color,
    urls::{pr_url, work_item_url},
    view::{TypeState, other_states},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    format::plural,
    host, messages,
    task::{join_all, spawn},
    ui::{Node, card, empty, heading, muted, page, row, stack, text},
};

/// The pull requests whose titles are asked for, one call each; the rest show by number, behind Show all.
const PRS_SHOWN: usize = 10;

struct Shown {
    id: u64,
    fields: Value,
    relations: Value,
    description: Vec<Block>,
    repro: Vec<Block>,
    acceptance: Vec<Block>,
}

#[derive(Clone)]
struct PrInfo {
    link: PrLink,
    title: Option<String>,
    status: Option<String>,
    repo: Option<String>,
}

/// A part of the page that loads on its own: still on its way, failed, or here.
enum Part<T> {
    Pending,
    Failed(String),
    Ready(T),
}

struct Updates {
    comments: Vec<Comment>,
    /// The lines of each comment's HTML, read once when the updates arrive.
    comment_blocks: Vec<Vec<Block>>,
    revisions: Vec<Revision>,
    feed: Vec<Activity>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Overview,
    Links,
    History,
}

struct Page {
    args: Option<ItemArgs>,
    item: Option<Shown>,
    /// Why the work item couldn't be read: the page's error card.
    error: Option<String>,
    states: Vec<TypeState>,
    linked: Part<HashMap<u64, LinkedItem>>,
    updates: Part<Updates>,
    prs: Vec<PrInfo>,
    /// The state a move is in flight to.
    moving: Option<String>,
    move_error: Option<String>,
    open_error: Option<String>,
    tab: Tab,
    all_prs: bool,
    menu_open: bool,
}

thread_local! {
    static PAGE: RefCell<Page> = const { RefCell::new(Page {
        args: None, item: None, error: None, states: Vec::new(), linked: Part::Pending, updates: Part::Pending, prs: Vec::new(), moving: None,
        move_error: None, open_error: None, tab: Tab::Overview, all_prs: false, menu_open: false,
    }) };
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

fn blocks(field: &Value) -> Vec<Block> {
    field.as_str().map(str::trim).filter(|html| !html.is_empty()).map(blocks_of).unwrap_or_default()
}

// ---------- loading ----------

/// Reads the work item, and starts reading its parts, which show as each one arrives. Done when the work item is here.
async fn load() {
    let Some(args) = with(|p| {
        p.error = None;
        p.args.clone()
    }) else {
        return;
    };
    let wi = match work_item(&args.org, args.id, false).await {
        Ok(Some(wi)) => wi,
        Ok(None) => return with(|p| p.error = Some(format!("Work item {} doesn't exist, or you can't see it.", args.id))),
        Err(error) => return with(|p| p.error = Some(error)),
    };
    let fields = wi["fields"].clone();
    let relations = wi["relations"].clone();
    let text = |name: &str| fields[name].as_str().unwrap_or_default().to_owned();
    let (project, kind) = (text("System.TeamProject"), text("System.WorkItemType"));
    let parent = fields["System.Parent"].as_u64();
    with(|p| {
        p.item = Some(Shown {
            id: wi["id"].as_u64().unwrap_or(args.id),
            description: blocks(&fields["System.Description"]),
            repro: blocks(&fields["Microsoft.VSTS.TCM.ReproSteps"]),
            acceptance: blocks(&fields["Microsoft.VSTS.Common.AcceptanceCriteria"]),
            fields,
            relations: relations.clone(),
        })
    });
    // The rest is independent: each part shows when it arrives, and one failing doesn't blank the others.
    let (org, id) = (args.org, args.id);
    spawn(load_states(org.clone(), project.clone(), kind));
    spawn(load_links(org.clone(), project.clone(), relations.clone(), parent));
    spawn(load_updates(org.clone(), project.clone(), id));
    spawn(load_prs(org, project, pull_request_links(&relations)));
}

async fn load_states(org: String, project: String, kind: String) {
    let args = invoke("wit", "workItemTypeStates", &[("project", &project), ("type", &kind)], &[], None);
    let states = az(&org, &args, false, Some(json!({ "value": [] }))).await.map(|answer| TypeState::list(&answer)).unwrap_or_default();
    with(|p| p.states = states);
}

async fn load_links(org: String, project: String, relations: Value, parent: Option<u64>) {
    let ids = all_link_ids(&link_ids(&relations, parent));
    if ids.is_empty() {
        return with(|p| p.linked = Part::Ready(HashMap::new()));
    }
    // One query for every linked work item, not a call each.
    let linked = match query(&org, &project, &ids_query(&ids)).await {
        Ok(rows) => Part::Ready(linked_items(&rows)),
        Err(error) => Part::Failed(error.message),
    };
    with(|p| p.linked = linked);
}

async fn load_updates(org: String, project: String, id: u64) {
    let args = invoke("wit", "updates", &[("project", &project), ("id", &id.to_string())], &[], None);
    let updates = match az(&org, &args, false, Some(json!({ "value": [] }))).await {
        Ok(answer) => {
            let list = answer["value"].as_array().cloned().unwrap_or_default();
            let comments = comments_of(&list);
            let revisions = revisions_of(&list);
            Part::Ready(Updates { comment_blocks: comments.iter().map(|c| blocks_of(&c.html)).collect(), feed: discussion_of(&comments, &revisions), comments, revisions })
        }
        Err(error) => Part::Failed(error.message),
    };
    with(|p| p.updates = updates);
}

async fn load_prs(org: String, project: String, links: Vec<PrLink>) {
    with(|p| p.prs = links.iter().map(|link| PrInfo { link: link.clone(), title: None, status: None, repo: None }).collect());
    // Titles for the first ten, one call each; the rest show by number, behind Show all.
    let asked: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = PrInfo>>>> = links
        .iter()
        .take(PRS_SHOWN)
        .map(|link| {
            let (link, org, project) = (link.clone(), org.clone(), project.clone());
            Box::pin(async move {
                let args = invoke("git", "pullRequests", &[("project", &project), ("pullRequestId", &link.id.to_string())], &[], None);
                match az(&org, &args, false, None).await {
                    Ok(pr) => PrInfo { link, title: pr["title"].as_str().map(str::to_owned), status: pr["status"].as_str().map(str::to_owned), repo: pr["repository"]["name"].as_str().map(str::to_owned) },
                    Err(_) => PrInfo { link, title: None, status: None, repo: None },
                }
            }) as _
        })
        .collect();
    let mut prs = join_all(asked).await;
    prs.extend(links.iter().skip(PRS_SHOWN).map(|link| PrInfo { link: link.clone(), title: None, status: None, repo: None }));
    with(|p| p.prs = prs);
}

fn reload() {
    spawn(load());
}

fn move_to(state: String) {
    let Some(args) = with(|p| {
        p.moving = Some(state.clone());
        p.move_error = None;
        p.menu_open = false;
        p.args.clone()
    }) else {
        return;
    };
    spawn(async move {
        let update = ["boards", "work-item", "update", "--id", &args.id.to_string(), "--state", &state].map(str::to_owned);
        match az_run(&args.org, &update, false).await {
            Ok(_) => {
                load().await;
                // The pane badge in main shows the new state.
                messages::tell("item-changed", &json!({ "org": args.org, "id": args.id }));
                with(|p| p.moving = None);
            }
            Err(error) => with(|p| {
                p.moving = None;
                p.move_error = Some(error.message);
            }),
        }
    });
}

fn open_url(url: String) {
    with(|p| p.open_error = None);
    spawn(async move {
        if let Err(error) = host::call("openUrl", json!({ "url": url })).await {
            with(|p| p.open_error = Some(error));
        }
    });
}

fn open_item(args: &ItemArgs, id: u64) {
    drop(host::open_view_with("item", "tab", &item_tab_args(id, &args.org)));
}

// ---------- the page ----------

fn button(label: &str, variant: &str, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").prop("variant", variant).prop("size", "sm").on("onClick", move |_| on_click()).text(label)
}

fn toolbar(id: u64, kind: Option<&str>, actions: Vec<Node>) -> Node {
    let node = Node::new("Toolbar").child(Node::new("Slot").prop("name", "title").text(format!("{} #{id}", kind.unwrap_or("Work item"))));
    if actions.is_empty() { node } else { node.child(Node::new("Slot").prop("name", "actions").children(actions)) }
}

/// A column at most 860 points wide, centred in the room it has.
fn wrap(content: Vec<Node>) -> Node {
    gap(stack(), "l").prop("maxWidth", 860).children(content)
}

fn view() -> Vec<Node> {
    PAGE.with(|page| build(&page.borrow()))
}

fn build(p: &Page) -> Vec<Node> {
    let Some(args) = &p.args else {
        return vec![page(Node::new("Toolbar").child(Node::new("Slot").prop("name", "title").text("Work item")), [empty("No work item", "Open a work item from a pane's badge or a project's Work items.")])];
    };
    if let Some(error) = &p.error {
        let card = card_of(vec![strong("Couldn't load this work item"), text(error.as_str()).prop("tone", "danger"), row().child(button("Try again", "outline", reload))]);
        return vec![page(toolbar(args.id, None, vec![]), [wrap(vec![card])])];
    }
    let Some(item) = &p.item else {
        let bones = vec![skeleton(json!("30%"), 14), skeleton(json!("80%"), 26), skeleton(json!("55%"), 14), skeleton(json!(null), 90), skeleton(json!(null), 120)];
        return vec![page(toolbar(args.id, None, vec![]), [wrap(vec![skeletons("Loading work item", bones)])])];
    };
    let f = &item.fields;
    let project = f["System.TeamProject"].as_str().unwrap_or_default();
    let kind = f["System.WorkItemType"].as_str().unwrap_or_default();
    let url = work_item_url(&args.org, project, args.id);
    let links = link_ids(&item.relations, f["System.Parent"].as_u64());
    let link_count = all_link_ids(&links).len() + p.prs.len();
    let comments = match &p.updates {
        Part::Ready(u) => Some(u.comments.len()),
        _ => None,
    };
    let parent_title = links.parent.and_then(|id| match &p.linked {
        Part::Ready(linked) => linked.get(&id).map(|l| l.title.clone()),
        _ => None,
    });
    let tabs = Node::new("Chips")
        .prop("label", "Work item")
        .prop("line", true)
        .prop("value", match p.tab { Tab::Overview => "overview", Tab::Links => "links", Tab::History => "history" })
        .prop(
            "options",
            json!([
                { "value": "overview", "label": format!("Overview{}", comments.filter(|n| *n > 0).map_or(String::new(), |n| format!(" {n}"))) },
                { "value": "links", "label": format!("Links{}", if link_count > 0 { format!(" {link_count}") } else { String::new() }) },
                { "value": "history", "label": format!("History{}", match &p.updates { Part::Ready(u) => format!(" {}", u.revisions.len()), _ => String::new() }) },
            ]),
        )
        .on("onChange", |event| {
            let tab = match event["value"].as_str() {
                Some("links") => Tab::Links,
                Some("history") => Tab::History,
                _ => Tab::Overview,
            };
            with(|p| p.tab = tab);
        });
    let panel = match p.tab {
        Tab::Overview => overview(p, item),
        Tab::Links => links_tab(p, args, project, &links),
        Tab::History => history_tab(p),
    };
    let failed = p.move_error.as_ref().or(p.open_error.as_ref());
    let content = gap(stack(), "l")
        .prop("maxWidth", 860)
        .child(header(p, item, args, links.parent, parent_title))
        .maybe(failed.map(|error| text(error.as_str()).prop("tone", "danger")))
        .child(summary(p, &links))
        .child(gap(stack(), "m").child(tabs).child(panel));
    let toolbar = toolbar(
        args.id,
        Some(kind),
        vec![button("Refresh", "outline", reload), Node::new("Button").prop("size", "sm").on("onClick", move |_| open_url(url.clone())).text("Open in Azure DevOps")],
    );
    vec![page(toolbar, [content])]
}

// ---------- header ----------

/// A work item state's tone, as the names a `Badge` and a `Dot` take.
fn tone_of(state: &str) -> Option<&'static str> {
    match state_tone(state) {
        "success" => Some("success"),
        "warning" => Some("warn"),
        "neutral" => None,
        _ => Some("info"),
    }
}

fn header(p: &Page, item: &Shown, args: &ItemArgs, parent_id: Option<u64>, parent_title: Option<String>) -> Node {
    let f = &item.fields;
    let text_of = |name: &str| f[name].as_str().unwrap_or_default();
    let (state, kind, project) = (text_of("System.State"), text_of("System.WorkItemType"), text_of("System.TeamProject"));
    let tone = tone_of(state);
    let who = f["System.AssignedTo"]["displayName"].as_str().filter(|who| !who.is_empty());
    let sprint = last_segment(f["System.IterationPath"].as_str());
    let area = last_segment(f["System.AreaPath"].as_str());
    let tags = tags_of(f["System.Tags"].as_str());
    let dot = tone_dot(if tone == Some("success") { None } else { Some(tone.unwrap_or("muted")) });
    let label = match &p.moving {
        Some(moving) => format!("Moving to {moving}…"),
        None => state.to_owned(),
    };
    let next = other_states(state, &p.states);
    let state_node = if next.is_empty() {
        badge("outline", tone).child(dot).text(label)
    } else {
        let menu = Node::new("Button")
            .prop("variant", "outline")
            .prop("size", "sm")
            .prop("label", format!("State {state}. Move this work item"))
            .on("onClick", |_| with(|p| p.menu_open = !p.menu_open));
        let menu = if p.moving.is_some() { menu.prop("disabled", true) } else { menu };
        let items = p.menu_open.then(|| {
            next.iter().map(|to| {
                let to = to.clone();
                let (key, label) = (to.clone(), format!("Move to {to}"));
                Node::new("Button").key(key).prop("variant", "ghost").on("onClick", move |_| move_to(to.clone())).text(label)
            })
        });
        gap(stack(), "xs").child(menu.child(dot).text(label)).children(items.into_iter().flatten())
    };
    let created = f["System.CreatedDate"].as_str().and_then(time_text).map(|ago| {
        let by = f["System.CreatedBy"]["displayName"].as_str().filter(|name| !name.is_empty()).map_or(" ".to_owned(), |name| format!(" by {name} "));
        muted(format!("created{by}{ago}")).prop("size", "small")
    });
    let updated = f["System.ChangedDate"].as_str().and_then(time_text).map(|ago| muted(format!("updated {ago}")).prop("size", "small"));
    let person = gap(row(), "s").prop("wrap", true).maybe(who.map(|who| avatar(who, true))).child(match who {
        Some(who) => strong(who),
        None => muted("Unassigned"),
    });
    let parent = parent_id.map(|id| {
        let args = args.clone();
        Node::new("Button").prop("variant", "link").prop("size", "xs").on("onClick", move |_| open_item(&args, id)).text(format!("Child of #{id}{}", parent_title.map_or(String::new(), |t| format!(" {t}"))))
    });
    gap(stack(), "s")
        .child(gap(row(), "s").prop("wrap", true).child(type_dot(type_color(kind))).child(text(kind)).maybe(parent))
        .child(large_title(f["System.Title"].as_str().unwrap_or_default()).child(muted(format!(" #{}", item.id))))
        .child(
            gap(row(), "s")
                .prop("wrap", true)
                .child(state_node)
                .child(person)
                .maybe((!sprint.is_empty()).then(|| pill(sprint, "outline", None)))
                .maybe((!area.is_empty() && area != project).then(|| pill(area, "secondary", None)))
                .maybe(created)
                .maybe(updated),
        )
        .maybe((!tags.is_empty()).then(|| gap(row(), "s").prop("wrap", true).children(tags.iter().map(|tag| gap(stack(), "s").key(tag.clone()).child(pill(tag.as_str(), "secondary", None))))))
}

// ---------- summary ----------

fn hours(n: f64) -> String {
    format!("{} h", if n.fract() == 0.0 { js_number(n) } else { format!("{n:.1}") })
}

fn summary(p: &Page, links: &LinkIds) -> Node {
    let Some(item) = &p.item else { return row() };
    let s = summary_of(&item.fields);
    let active = p.prs.iter().filter(|pr| pr.status.as_deref() == Some("active")).count();
    let go = || with(|p| p.tab = Tab::Links);
    let mut chips = Vec::new();
    if let Some(priority) = s.priority {
        chips.push(pill(format!("Priority {}", js_number(priority)), "outline", if priority == 1.0 { Some("danger") } else if priority == 2.0 { Some("warn") } else { None }));
    }
    if let Some(severity) = s.severity {
        chips.push(pill(format!("{severity} severity"), "outline", None));
    }
    if let Some((label, value)) = s.estimate {
        chips.push(pill(format!("{} {}", js_number(value), label.to_lowercase()), "outline", None));
    }
    if let Some(remaining) = s.remaining {
        chips.push(pill(format!("{} left", hours(remaining)), "outline", None));
    }
    if p.prs.is_empty() {
        chips.push(pill("No pull requests", "outline", None));
    } else {
        let tone = if active > 0 { Some("info") } else if p.prs.iter().all(|pr| pr.status.as_deref() == Some("completed")) { Some("success") } else { None };
        let label = format!("{}{}", plural(p.prs.len(), "pull request"), if active > 0 { format!(" · {active} active") } else { String::new() });
        chips.push(pill_button(label, tone, go));
    }
    if !links.children.is_empty() {
        let done = match &p.linked {
            Part::Ready(linked) => Some(links.children.iter().filter(|id| linked.get(id).is_some_and(|l| state_tone(&l.state) == "success")).count()),
            _ => None,
        };
        let count = links.children.len();
        let label = format!("{count} {}{}", if count == 1 { "child" } else { "children" }, done.map_or(String::new(), |done| format!(" · {done} done")));
        chips.push(pill_button(label, None, go));
    }
    gap(row(), "m").prop("wrap", true).children(chips.into_iter().enumerate().map(|(at, chip)| chip.key(at.to_string())))
}

// ---------- overview ----------

fn loading(label: &str) -> Node {
    skeletons(label, vec![skeleton(Value::Null, 80), skeleton(Value::Null, 80)])
}

fn part_error(message: &str) -> Node {
    card_of(vec![text(message).prop("tone", "danger"), row().child(button("Try again", "outline", reload))])
}

fn section(label: &str, blocks: &[Block]) -> Node {
    card_of(vec![strong(label), crate::parts::html(blocks)])
}

fn overview(p: &Page, item: &Shown) -> Node {
    let f = &item.fields;
    let author = f["System.CreatedBy"]["displayName"].as_str().filter(|name| !name.is_empty());
    let head = author.map(|author| {
        let created = f["System.CreatedDate"].as_str().and_then(time_text).unwrap_or_default();
        gap(row(), "s").prop("wrap", true).child(avatar(author, false)).child(strong(author)).child(muted(format!("created this {created}")).prop("size", "small"))
    });
    let description = if item.description.is_empty() { muted("No description provided.") } else { crate::parts::html(&item.description) };
    let comments = match &p.updates {
        Part::Ready(u) => Some(u.comments.len()),
        _ => None,
    };
    let discussion = match &p.updates {
        Part::Failed(error) => part_error(error),
        Part::Pending => loading("Loading discussion"),
        Part::Ready(u) if !u.feed.iter().any(|a| matches!(a, Activity::Comment { .. })) && u.feed.len() <= 1 => empty("No comments yet", "Comments and changes to the state, assignee and sprint show up here."),
        Part::Ready(u) => gap(stack(), "m").children(u.feed.iter().map(|activity| activity_node(activity, u, author))),
    };
    gap(stack(), "l")
        .child(card().child(gap(stack(), "m").maybe(head).child(description)))
        .maybe((!item.repro.is_empty()).then(|| section("Repro steps", &item.repro)))
        .maybe((!item.acceptance.is_empty()).then(|| section("Acceptance criteria", &item.acceptance)))
        .child(heading(3, format!("Discussion{}", comments.map_or(String::new(), |n| format!(" · {n}")))))
        .child(discussion)
}

/// "changed state from New to Active", "set sprint to Sprint 5", "cleared assigned to".
fn describe(note: Node, c: &Change) -> Node {
    let what = c.label.to_lowercase();
    match (c.label == "Assigned to", c.from.as_deref(), c.to.as_deref()) {
        (true, Some(from), None) => note.text("unassigned ").child(strong(from)),
        (true, None, Some(to)) => note.text("assigned to ").child(strong(to)),
        (true, Some(from), Some(to)) => note.text("reassigned from ").child(strong(from)).text(" to ").child(strong(to)),
        (_, Some(from), None) => note.text(format!("cleared {what} (was ")).child(strong(from)).text(")"),
        (_, None, Some(to)) => note.text(format!("set {what} to ")).child(strong(to)),
        (_, Some(from), Some(to)) => note.text(format!("changed {what} from ")).child(strong(from)).text(" to ").child(strong(to)),
        (_, None, None) => note,
    }
}

fn activity_node(activity: &Activity, updates: &Updates, author: Option<&str>) -> Node {
    match activity {
        Activity::Comment { key, author: by, at, comment } => card().key(key.clone()).child(
            gap(stack(), "s")
                .child(
                    gap(row(), "s")
                        .child(avatar(by, true))
                        .child(strong(by.as_str()))
                        .maybe((Some(by.as_str()) == author).then(|| pill("Author", "secondary", None)))
                        .maybe(time_text(at).map(|ago| muted(ago).prop("size", "small").prop("grow", true))),
                )
                .child(crate::parts::html(&updates.comment_blocks[*comment])),
        ),
        Activity::Change { key, author: by, at, created, changes } => {
            let mut note = Node::new("Text").prop("tone", "muted");
            if *created {
                note = note.text("created this work item");
            }
            for (at, change) in changes.iter().filter(|_| !*created).enumerate() {
                note = describe(if at > 0 { note.text(", ") } else { note }, change);
            }
            gap(row(), "s")
                .prop("wrap", true)
                .key(key.clone())
                .child(avatar(by, true))
                .child(strong(by.as_str()))
                .child(note)
                .maybe(time_text(at).map(|ago| muted(ago).prop("size", "small")))
        }
    }
}

// ---------- links ----------

fn link_row(args: &ItemArgs, id: u64, item: Option<&LinkedItem>) -> Node {
    let label = format!("Open #{id}{}", item.map_or(String::new(), |item| format!(" {}", item.title)));
    let args = args.clone();
    let button = Node::new("ListButton")
        .prop("title", label)
        .on("onClick", move |_| open_item(&args, id))
        .maybe(item.map(|item| type_dot(type_color(&item.kind))))
        .child(muted(format!("#{id}")))
        .child(Node::new("Text").prop("grow", true).text(item.map_or("", |item| item.title.as_str())))
        .maybe(item.filter(|item| !item.state.is_empty()).map(|item| pill(item.state.as_str(), "outline", tone_of(&item.state))))
        .maybe(item.and_then(|item| item.assignee.as_deref()).map(|who| Node::new("Text").child(avatar(who, true))));
    gap(stack(), "s").key(id.to_string()).child(button)
}

fn pr_row(args: &ItemArgs, project: &str, pr: &PrInfo) -> Node {
    let label = format!("Open pull request {}{} in the browser", pr.link.id, pr.title.as_ref().map_or(String::new(), |title| format!(" {title}")));
    let url = pr_url(&args.org, project, &pr.link.repo_id, pr.link.id);
    let status = pr_status(pr.status.as_deref());
    let button = Node::new("ListButton")
        .prop("title", label)
        .on("onClick", move |_| open_url(url.clone()))
        .child(muted(format!("PR #{}", pr.link.id)))
        .child(Node::new("Text").prop("grow", true).text(pr.title.as_deref().unwrap_or_default()))
        .maybe(pr.repo.as_ref().map(|repo| muted(repo.as_str()).prop("size", "small")))
        .maybe((!status.0.is_empty()).then(|| pill(status.0, "outline", status.1)));
    gap(stack(), "s").key(pr.link.id.to_string()).child(button)
}

fn links_tab(p: &Page, args: &ItemArgs, project: &str, links: &LinkIds) -> Node {
    let groups = [
        ("Parent", links.parent.into_iter().collect::<Vec<u64>>()),
        ("Children", links.children.clone()),
        ("Related", links.related.clone()),
        ("Predecessors", links.predecessors.clone()),
        ("Successors", links.successors.clone()),
    ];
    let linked_any = groups.iter().any(|(_, ids)| !ids.is_empty());
    if !linked_any && p.prs.is_empty() {
        return gap(stack(), "l").child(empty("No links", "Parent and child work items, related items and pull requests show up here."));
    }
    let waiting = linked_any && matches!(p.linked, Part::Pending);
    let linked = match &p.linked {
        Part::Ready(linked) => Some(linked),
        _ => None,
    };
    let shown = if p.all_prs { p.prs.len() } else { PRS_SHOWN.min(p.prs.len()) };
    gap(stack(), "l")
        .maybe(match &p.linked {
            Part::Failed(error) => Some(part_error(error)),
            _ => None,
        })
        .children(groups.iter().filter(|(_, ids)| !ids.is_empty()).map(|(title, ids)| {
            let rows = if waiting {
                skeletons(&format!("Loading {}", title.to_lowercase()), ids.iter().map(|_| skeleton(Value::Null, 36)).collect())
            } else {
                gap(stack(), "s").children(ids.iter().map(|id| link_row(args, *id, linked.and_then(|linked| linked.get(id)))))
            };
            gap(stack(), "s").key(*title).child(heading(3, format!("{title} · {}", ids.len()))).child(rows)
        }))
        .maybe((!p.prs.is_empty()).then(|| {
            gap(stack(), "s")
                .child(heading(3, format!("Pull requests · {}", p.prs.len())))
                .child(gap(stack(), "s").children(p.prs.iter().take(shown).map(|pr| pr_row(args, project, pr))))
                .maybe((p.prs.len() > PRS_SHOWN && !p.all_prs).then(|| {
                    Node::new("Button").prop("variant", "link").prop("size", "sm").on("onClick", |_| with(|p| p.all_prs = true)).text(format!("Show all {}", p.prs.len()))
                }))
        }))
}

// ---------- history ----------

fn revision_node(r: &Revision) -> Node {
    let changes = r.changes.iter().map(|c| {
        gap(row(), "s")
            .prop("wrap", true)
            .key(c.label)
            .child(muted(c.label))
            .maybe(c.from.as_ref().map(|from| muted(from.as_str())))
            .maybe(c.from.as_ref().zip(c.to.as_ref()).map(|_| muted("→")))
            .child(match &c.to {
                Some(to) => strong(to.as_str()),
                None => muted("cleared"),
            })
    });
    let head = gap(row(), "s").prop("wrap", true).child(strong(r.by.as_str())).maybe(r.created.then(|| muted("created this work item").prop("size", "small"))).maybe(time_text(&r.at).map(text));
    gap(row(), "s")
        .prop("align", "start")
        .key(r.id.to_string())
        .child(avatar(&r.by, true))
        .child(gap(stack(), "s").prop("grow", true).child(head).maybe((!r.changes.is_empty()).then(|| gap(stack(), "s").children(changes))))
}

fn history_tab(p: &Page) -> Node {
    let column = gap(stack(), "l");
    match &p.updates {
        Part::Failed(error) => column.child(part_error(error)),
        Part::Pending => column.child(loading("Loading history")),
        Part::Ready(u) if u.revisions.is_empty() => column.child(empty("No history", "State, assignee and sprint changes show up here.")),
        Part::Ready(u) => column.child(gap(stack(), "s").children(u.revisions.iter().rev().map(revision_node))),
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    with(|p| p.args = host::view_args().as_ref().and_then(item_args));
    wings_plugin::run_filled(view);
    if with(|p| p.args.is_some()) {
        messages::on("item-changed", |_| reload());
        reload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::task::block_on;

    const ORG: &str = "https://dev.azure.com/acme";

    /// A fake `az` that answers like the project `Web` does for work item 1432, and records what it was asked.
    fn az_answering(missing: bool, update_fails: bool) -> Rc<RefCell<Vec<Vec<String>>>> {
        let asked: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let seen = asked.clone();
        host::testing::answer(move |_, params| {
            let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect();
            seen.borrow_mut().push(args.clone());
            let ok = |answer: Value| Ok(json!({ "code": 0, "stdout": answer.to_string(), "stderr": "" }));
            let has = |word: &str| args.iter().any(|a| a == word);
            if args[0] == "rest" {
                let value = if missing {
                    vec![Value::Null]
                } else {
                    vec![json!({
                        "id": 1432,
                        "fields": { "System.Title": "Sync gives up", "System.TeamProject": "Web", "System.WorkItemType": "Bug", "System.State": "Active", "System.Description": "<p>It stops.</p>", "System.Parent": 1400 },
                        "relations": [
                            { "rel": "System.LinkTypes.Hierarchy-Forward", "url": "https://dev.azure.com/acme/_apis/wit/workItems/1433" },
                            { "rel": "ArtifactLink", "url": "vstfs:///Git/PullRequestId/p%2Fr1%2F482", "attributes": { "name": "Pull Request" } },
                        ],
                    })]
                };
                return ok(json!({ "value": value }));
            }
            if has("update") && args[0] == "boards" {
                return if update_fails { Ok(json!({ "code": 1, "stdout": "", "stderr": "ERROR: TF401320: Rule Error\n" })) } else { ok(json!({})) };
            }
            if args[0] == "boards" {
                return ok(json!([{ "id": 1400, "fields": { "System.Title": "Make it reliable", "System.State": "Active", "System.WorkItemType": "Feature" } }, { "id": 1433, "fields": { "System.Title": "Retry", "System.State": "New", "System.WorkItemType": "Task" } }]));
            }
            if has("workItemTypeStates") {
                return ok(json!({ "value": [{ "name": "New", "category": "Proposed" }, { "name": "Active", "category": "InProgress" }, { "name": "Closed", "category": "Completed" }] }));
            }
            if has("updates") {
                return ok(json!({ "value": [
                    { "id": 1, "revisedBy": { "displayName": "Grace" }, "fields": { "System.ChangedDate": { "newValue": "2026-10-31T09:00:00Z" }, "System.State": { "newValue": "New" } } },
                    { "id": 2, "revisedBy": { "displayName": "Ada" }, "fields": { "System.ChangedDate": { "newValue": "2026-11-01T09:00:00Z" }, "System.History": { "newValue": "<p>Reproduced.</p>" } } },
                ] }));
            }
            if has("pullRequests") {
                return ok(json!({ "title": "Add retry", "status": "active", "repository": { "name": "web" } }));
            }
            Err(format!("no answer for {args:?}"))
        });
        asked
    }

    fn opened() {
        with(|p| p.args = Some(ItemArgs { id: 1432, org: ORG.into() }));
    }

    #[test]
    fn a_work_item_loads_with_its_states_links_comments_and_pull_requests_each_on_its_own() {
        let asked = az_answering(false, false);
        opened();
        block_on(load());
        with(|p| {
            assert_eq!(p.item.as_ref().map(|i| i.id), Some(1432));
            assert_eq!(p.states.len(), 3);
            let Part::Ready(linked) = &p.linked else { panic!("the links are here") };
            assert_eq!((linked[&1400].title.as_str(), linked[&1433].state.as_str()), ("Make it reliable", "New"));
            let Part::Ready(updates) = &p.updates else { panic!("the updates are here") };
            assert_eq!((updates.comments.len(), updates.revisions.len(), updates.feed.len()), (1, 1, 2));
            assert_eq!(updates.comment_blocks[0][0].text(), "Reproduced.");
            assert_eq!((p.prs.len(), p.prs[0].title.as_deref(), p.prs[0].status.as_deref(), p.prs[0].repo.as_deref()), (1, Some("Add retry"), Some("active"), Some("web")));
            assert_eq!(p.item.as_ref().unwrap().description[0].text(), "It stops.");
            assert!(p.error.is_none());
        });
        // The parent and the child are one query, not a call each.
        assert_eq!(asked.borrow().iter().filter(|args| args[0] == "boards").count(), 1);
    }

    #[test]
    fn a_work_item_that_isnt_there_is_the_pages_error() {
        az_answering(true, false);
        opened();
        block_on(load());
        assert_eq!(with(|p| p.error.clone()).as_deref(), Some("Work item 1432 doesn't exist, or you can't see it."));
    }

    #[test]
    fn a_move_runs_az_and_reads_the_work_item_again_and_a_refused_one_shows_its_error_over_the_item() {
        let asked = az_answering(false, false);
        opened();
        block_on(load());
        asked.borrow_mut().clear();
        move_to("Closed".into());
        block_on(async {});
        assert!(asked.borrow().iter().any(|args| args[..7] == ["boards", "work-item", "update", "--id", "1432", "--state", "Closed"]));
        assert!(asked.borrow().iter().any(|args| args[0] == "rest"));
        assert_eq!(with(|p| (p.moving.clone(), p.move_error.clone())), (None, None));
        az_answering(false, true);
        move_to("Closed".into());
        block_on(async {});
        assert_eq!(with(|p| (p.moving.clone(), p.move_error.clone())), (None, Some("ERROR: TF401320: Rule Error".into())));
    }
}
