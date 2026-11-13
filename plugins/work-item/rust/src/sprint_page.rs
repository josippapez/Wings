//! The Sprint block of a Project page: the work items of the sprint running now in the project's Azure DevOps project, a
//! column per state. It runs the queries itself, again every minute while Wings is in front, and a click opens the work
//! item in a tab.

use crate::{
    az::{query, remote_for, sprint_paths},
    item::item_tab_args,
    parts::{avatar, empty_title, gap, type_dot},
    sections::{BlockItem, assigned_query, block_items, clean_block_state, group_by_state, sprint_query},
    type_color::type_color,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use wings_plugin::{
    format::truncate,
    host, poll,
    task::spawn,
    ui::{Node, card, empty, muted, row, stack, text},
};

/// How often the sprint is read again while Wings is in front.
const REFETCH_MS: u32 = 60_000;

/// The running sprints of an Azure DevOps project and their work items: `items` is empty with no running sprint, and
/// `total` counts past the cap.
struct Sprint {
    org: String,
    sprints: Vec<String>,
    items: Vec<BlockItem>,
    total: usize,
}

/// `None` is a project that isn't on Azure DevOps.
type Answer = Option<Sprint>;

struct Page {
    /// Until the app says which project it is, nothing; then the project's folder and id, or none when none is open.
    project: Option<Option<(String, String)>>,
    /// What the block saved: only what's assigned to you, or the whole sprint. Nothing until it is read.
    mine: Option<bool>,
    answer: Option<Result<Answer, String>>,
    /// Goes up with each read: an older one that finds a later number drops out.
    reads: u64,
}

thread_local! {
    static PAGE: RefCell<Page> = const { RefCell::new(Page { project: None, mine: None, answer: None, reads: 0 }) };
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

async fn read_sprint(path: &str, mine: bool) -> Result<Answer, String> {
    let Some(remote) = remote_for(path).await else { return Ok(None) };
    let (org, project) = (remote.org, remote.project);
    let paths = sprint_paths(&org, &project).await?;
    if paths.is_empty() {
        return Ok(Some(Sprint { org, sprints: Vec::new(), items: Vec::new(), total: 0 }));
    }
    let wiql = if mine { assigned_query(&project, &paths) } else { sprint_query(&project, &paths) };
    let rows = query(&org, &project, &wiql).await.map_err(String::from)?;
    Ok(Some(Sprint { org, sprints: paths.iter().map(|p| p.rsplit('\\').next().unwrap_or(p).to_owned()).collect(), items: block_items(&rows), total: rows.len() }))
}

/// Reads the sprint of the project on screen, for the filter. The answer on screen stays until it comes unless `clear`.
fn read(clear: bool) {
    let Some((path, mine, number)) = with(|p| {
        let (Some(Some((path, _))), Some(mine)) = (&p.project, p.mine) else { return None };
        if clear {
            p.answer = None;
        }
        p.reads += 1;
        Some((path.clone(), mine, p.reads))
    }) else {
        return;
    };
    spawn(async move {
        let answer = read_sprint(&path, mine).await;
        with(|p| {
            if p.reads == number {
                p.answer = Some(answer);
            }
        });
    });
}

fn set_mine(mine: bool) {
    with(|p| p.mine = Some(mine));
    drop(host::set_block_state(&json!({ "mine": mine })));
    read(true);
}

fn open_item(org: &str, id: u64) {
    drop(host::open_view_with("item", "tab", &item_tab_args(id, org)));
}

fn item_card(item: &BlockItem, org: &str, show_assignee: bool) -> Node {
    let label = format!("Open {} {}: {}{}", item.kind, item.id, item.title, item.assignee.as_ref().filter(|_| show_assignee).map_or(String::new(), |who| format!(", assigned to {who}")));
    let (org, id) = (org.to_owned(), item.id);
    let body = gap(stack(), "xs")
        .child(text(item.title.as_str()).prop("weight", "medium").prop("clamp", true))
        .child(gap(row(), "s").child(type_dot(type_color(&item.kind))).child(muted(format!("#{}", item.id)).prop("size", "tiny").prop("numeric", true)).maybe(
            item.assignee.as_ref().filter(|_| show_assignee).map(|who| gap(row(), "s").prop("wrap", true).child(avatar(who, true))),
        ));
    card().key(item.id.to_string()).child(Node::new("ListButton").prop("title", label).on("onClick", move |_| open_item(&org, id)).child(body))
}

fn column(state: &str, items: &[BlockItem], org: &str, show_assignee: bool) -> Node {
    Node::new("BoardColumn")
        .key(state)
        .prop("count", items.len())
        .child(Node::new("Slot").prop("name", "title").text(state))
        .child(Node::new("Slot").prop("name", "body").children(items.iter().map(|item| item_card(item, org, show_assignee))))
}

fn loading() -> Node {
    Node::new("Loading").text("Loading sprint")
}

fn view() -> Vec<Node> {
    PAGE.with(|page| build(&page.borrow()))
}

fn build(p: &Page) -> Vec<Node> {
    let (Some(project), Some(mine)) = (&p.project, p.mine) else {
        // The project not being known yet is the loading; with none open there is nothing to show.
        return vec![if matches!(p.project, Some(None)) { empty_title("No project") } else { loading() }];
    };
    if project.is_none() {
        return vec![empty_title("No project")];
    }
    // Not an Azure project: nothing to filter, so no chips.
    if matches!(p.answer, Some(Ok(None))) {
        return vec![muted("Not an Azure DevOps project.")];
    }
    let sprint = match &p.answer {
        Some(Ok(Some(sprint))) => Some(sprint),
        _ => None,
    };
    let names = sprint.map(|s| s.sprints.join(", ")).unwrap_or_default();
    let filter = Node::new("Chips")
        .prop("label", "Show")
        .prop("value", if mine { "mine" } else { "all" })
        .prop("options", json!([{ "value": "mine", "label": "Mine" }, { "value": "all", "label": "Whole sprint" }]))
        .on("onChange", |event| set_mine(event["value"] == "mine"));
    let body = match (&p.answer, sprint) {
        (Some(Err(error)), _) => gap(stack(), "s").child(empty("Couldn't load the sprint", &truncate(error, 300))),
        (None, _) | (_, None) => loading(),
        (_, Some(s)) if s.sprints.is_empty() => empty_title("No sprint running"),
        (_, Some(s)) if s.items.is_empty() => empty(if mine { "Nothing assigned to you" } else { "No work items" }, &names),
        (_, Some(s)) => {
            let columns = group_by_state(&s.items);
            let more = s.total.saturating_sub(s.items.len());
            gap(stack(), "s")
                .child(Node::new("Board").prop("label", format!("Work items in {names}")).children(columns.iter().map(|c| column(&c.state, &c.items, &s.org, !mine))))
                .maybe((more > 0).then(|| muted(format!("{more} more not shown")).prop("size", "tiny")))
        }
    };
    let shown = if matches!(p.answer, Some(Err(_))) { String::new() } else { names };
    vec![gap(stack(), "s").child(gap(row(), "s").prop("justify", "between").child(muted(shown).prop("size", "small").prop("truncate", true)).child(filter)).child(body)]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    host::on("project", |_, project| {
        let now = project["path"].as_str().map(|path| (path.to_owned(), project["id"].as_str().unwrap_or_default().to_owned()));
        // Another project starts over, as its page is its own.
        let changed = with(|p| {
            let changed = p.project.as_ref() != Some(&now);
            if changed {
                p.project = Some(now);
                p.answer = None;
            }
            changed
        });
        if changed {
            read(true);
        }
    });
    spawn(async {
        let saved = host::block_state().await.unwrap_or(Value::Null);
        with(|p| p.mine = Some(clean_block_state(&saved)));
        read(true);
    });
    poll::while_focused(REFETCH_MS, false, || read(false));
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::task::block_on;

    /// A fake `git` and `az` for a folder in `acme/Web`, whose Sprint 14 runs the whole of the test clock's day.
    fn answering(remote: &'static str) {
        host::testing::answer(move |_, params| {
            let args: Vec<&str> = params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
            let ok = |answer: Value| Ok(json!({ "code": 0, "stdout": if answer.is_string() { answer } else { answer.to_string().into() }, "stderr": "" }));
            if params["program"] == "git" {
                return ok(json!(remote));
            }
            if args.contains(&"classificationNodes") {
                return ok(json!({ "path": "\\Web\\Iteration", "children": [{ "path": "\\Web\\Iteration\\Sprint 14", "attributes": { "startDate": "1969-12-30T00:00:00Z", "finishDate": "1970-01-05T00:00:00Z" } }] }));
            }
            let wiql = args[args.iter().position(|a| *a == "--wiql").unwrap() + 1];
            let all = [(1, "Active", Some("Ada")), (2, "New", None), (3, "Closed", Some("Alan"))];
            let rows: Vec<Value> = all
                .iter()
                .filter(|(_, _, who)| !wiql.contains("@Me") || who == &Some("Ada"))
                .map(|(id, state, who)| json!({ "id": id, "fields": { "System.Title": format!("Item {id}"), "System.State": state, "System.WorkItemType": "Task", "System.AssignedTo": who.map(|name| json!({ "displayName": name })) } }))
                .collect();
            ok(json!(rows))
        });
    }

    #[test]
    fn the_sprint_is_what_is_assigned_to_you_or_the_whole_of_it_and_a_folder_that_isnt_azure_is_none() {
        answering("git@ssh.dev.azure.com:v3/acme/Web/web");
        let Ok(Some(mine)) = block_on(read_sprint("/work/web", true)) else { panic!("a sprint") };
        assert_eq!((mine.sprints.as_slice(), mine.items.iter().map(|i| i.id).collect::<Vec<_>>(), mine.total), (["Sprint 14".to_owned()].as_slice(), vec![1], 1));
        let Ok(Some(all)) = block_on(read_sprint("/work/web", false)) else { panic!("a sprint") };
        assert_eq!(all.items.iter().map(|i| (i.id, i.state.as_str())).collect::<Vec<_>>(), [(1, "Active"), (2, "New"), (3, "Closed")]);
        assert_eq!(all.org, "https://dev.azure.com/acme");
        answering("git@github.com:acme/web.git");
        assert!(matches!(block_on(read_sprint("/work/other", true)), Ok(None)));
    }

    #[test]
    fn a_read_that_fails_is_the_blocks_error_and_an_older_read_never_replaces_a_newer_one() {
        host::testing::answer(|_, params| if params["program"] == "git" { Ok(json!({ "code": 0, "stdout": "git@ssh.dev.azure.com:v3/acme/Web/web", "stderr": "" })) } else { Ok(json!({ "code": 1, "stdout": "", "stderr": "ERROR: boom\n" })) });
        with(|p| {
            p.project = Some(Some(("/work/web".into(), "/work/web".into())));
            p.mine = Some(true);
        });
        read(true);
        read(true);
        block_on(async {});
        with(|p| {
            assert_eq!(p.reads, 2);
            assert!(matches!(&p.answer, Some(Err(error)) if error == "ERROR: boom"));
        });
    }
}
