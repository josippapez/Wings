//! The main script: the badge on each pane whose branch names a work item, like `feature/1234-login`, with its state,
//! sprint and parent and actions to open it and to move it to its next state, and the Work items section under each
//! project in Wings' sidebar. The pages run `az` themselves; the item tab tells this script when it moved a work item.

use crate::{
    az::{az, az_run, cli, invoke, remote_for, work_item},
    branch::work_item_id,
    item::item_tab_args,
    project_sections,
    view::{Item, Ref, TypeState, WorkItem, badge_for, drop_closed, next_states},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
};
use wings_plugin::{
    cache::Cached,
    cli::{ProblemText, last_line},
    git::{ok, on_change, watched_project},
    host, messages, poll,
    task::{join, join_all, spawn},
};

const BRANCH_POLL_MS: u32 = 15_000;
const ITEM_POLL_MS: u32 = 60_000;
const PROBLEM_TEXT: ProblemText = ProblemText {
    title: "Azure DevOps",
    signed_out: "Sign in to Azure DevOps to see this branch's work item.",
    missing: "Work item needs the Azure CLI (az) to show this branch's work item.",
};

#[derive(Default)]
struct Badges {
    panes: Vec<Value>,
    /// paneId → the project whose repo the pane is in, or none when no project has it, so Wings doesn't watch it.
    watched: HashMap<String, Option<String>>,
    refs: HashMap<String, Ref>,
    /// `${org} ${id}` → the normalized work item, none there, or why it couldn't be read.
    items: HashMap<String, Item>,
    in_flight: HashSet<String>,
    /// Items refreshed because you clicked something; background polls don't show a spinner.
    manual: HashSet<String>,
}

thread_local! {
    static BADGES: RefCell<Badges> = RefCell::new(Badges::default());
    /// A parent's title, read once.
    static PARENT_TITLES: Cached<(String, u64), String> = Cached::new(f64::INFINITY, 0.0, |(org, id): (String, u64)| async move {
        Ok(work_item(&org, id, false).await?.and_then(|parent| parent["fields"]["System.Title"].as_str().map(str::to_owned)).unwrap_or_default())
    });
    /// A work item type's states, in workflow order, read once.
    static TYPE_STATES: Cached<(String, String, String), Vec<TypeState>> = Cached::new(f64::INFINITY, 0.0, |(org, project, kind): (String, String, String)| async move {
        let args = invoke("wit", "workItemTypeStates", &[("project", &project), ("type", &kind)], &[], None);
        Ok(TypeState::list(&az(&org, &args, false, Some(json!({ "value": [] }))).await?))
    });
}

fn with<T>(change: impl FnOnce(&mut Badges) -> T) -> T {
    BADGES.with(|badges| change(&mut badges.borrow_mut()))
}

// ---------- finding the work item for a pane ----------

/// Which panes to read the branch of.
enum Which {
    All,
    /// The panes in the repos of these projects, which Wings said changed.
    Projects(Vec<String>),
    /// The panes in a repo that is no project's: Wings doesn't watch it, so a poll reads it.
    Unwatched,
}

type Task = Pin<Box<dyn Future<Output = ()>>>;

async fn refresh_pane(pane: Value, projects: Value) {
    let (id, cwd) = (pane["paneId"].as_str().unwrap_or_default().to_owned(), pane["cwd"].as_str().unwrap_or_default().to_owned());
    let project = watched_project(&cwd, &projects).await;
    with(|b| b.watched.insert(id.clone(), project));
    let remote = remote_for(&cwd).await;
    let work_item = match &remote {
        Some(_) => work_item_id(&ok(&cwd, &["branch", "--show-current"]).await.unwrap_or_default()),
        None => None,
    };
    let (Some(remote), Some(work_item)) = (remote, work_item) else {
        with(|b| b.refs.remove(&id));
        return;
    };
    let r = Ref::new(&remote.org, &work_item);
    let key = r.key.clone();
    let unknown = with(|b| {
        b.refs.insert(id, r);
        !b.items.contains_key(&key)
    });
    if unknown {
        spawn(refresh_item(key, false));
    }
}

/// Reads the branch of the panes `which` picks.
async fn refresh_refs(which: Which) {
    let projects = host::projects().await.unwrap_or_default();
    let panes: Vec<Value> = with(|b| {
        let watched = |pane: &Value| b.watched.get(pane["paneId"].as_str().unwrap_or_default()).cloned().flatten();
        b.panes
            .iter()
            .filter(|pane| match &which {
                Which::All => true,
                Which::Projects(ids) => watched(pane).is_some_and(|project| ids.contains(&project)),
                Which::Unwatched => watched(pane).is_none(),
            })
            .cloned()
            .collect()
    });
    join_all(panes.into_iter().map(|pane| Box::pin(refresh_pane(pane, projects.clone())) as Task).collect()).await;
    with(|b| {
        let open: Vec<String> = b.panes.iter().filter_map(|pane| pane["paneId"].as_str().map(str::to_owned)).collect();
        let Badges { refs, items, .. } = b;
        drop_closed(refs, items, &open);
    });
    render();
}

// ---------- the work item ----------

async fn refresh_item(key: String, by_user: bool) {
    let started = with(|b| {
        let r = b.refs.values().find(|r| r.key == key).cloned()?;
        if by_user {
            b.manual.insert(key.clone());
        }
        b.in_flight.insert(key.clone()).then_some(r)
    });
    let Some(r) = started else {
        return;
    };
    render();
    let item = match load_item(&r).await {
        Ok(Some(found)) => Item::Found(found),
        Ok(None) => Item::Missing,
        Err(error) => Item::Error(Some(last_line(&error)).filter(|line| !line.is_empty()).unwrap_or("az failed").to_owned()),
    };
    with(|b| {
        b.items.insert(key.clone(), item);
        b.in_flight.remove(&key);
        b.manual.remove(&key);
    });
    render();
}

/// The work item, or none when there is none.
async fn load_item(r: &Ref) -> Result<Option<WorkItem>, String> {
    let Some(id) = r.id.parse().ok() else { return Ok(None) };
    let Some(wi) = work_item(&r.org, id, false).await? else { return Ok(None) };
    let f = &wi["fields"];
    let text = |name: &str| f[name].as_str().unwrap_or_default().to_owned();
    let (project, kind, state) = (text("System.TeamProject"), text("System.WorkItemType"), text("System.State"));
    let parent_id = f["System.Parent"].as_u64().filter(|id| *id > 0);
    let (states, parent) = join(
        async { TYPE_STATES.with(Cached::clone).get((r.org.clone(), project.clone(), kind.clone())).await.unwrap_or_default() },
        async {
            match parent_id {
                Some(id) => Some(PARENT_TITLES.with(Cached::clone).get((r.org.clone(), id)).await.unwrap_or_default()),
                None => None,
            }
        },
    )
    .await;
    let iteration = text("System.IterationPath");
    Ok(Some(WorkItem {
        title: text("System.Title"),
        category: states.iter().find(|s| s.name == state).map(|s| s.category.clone()),
        state,
        kind,
        states,
        sprint: if iteration.contains('\\') { iteration.rsplit('\\').next().unwrap_or_default().to_owned() } else { "None".into() },
        assigned: f["System.AssignedTo"]["displayName"].as_str().unwrap_or("Unassigned").to_owned(),
        parent: parent_id.map(|id| format!("#{id} {}", parent.unwrap_or_default()).trim().to_owned()),
        url: crate::urls::work_item_url(&r.org, &project, wi["id"].as_u64().unwrap_or(id)),
    }))
}

/// The one write: a failure shows in the card, and doesn't count as a sign-in problem for the org.
async fn move_to(r: &Ref, state: &str) -> Result<(), String> {
    with(|b| b.manual.insert(r.key.clone()));
    render();
    let args = ["boards", "work-item", "update", "--id", &r.id, "--state", state].map(str::to_owned);
    if let Err(error) = az_run(&r.org, &args, true).await {
        with(|b| b.manual.remove(&r.key));
        render();
        return Err(error.message);
    }
    refresh_item(r.key.clone(), true).await;
    Ok(())
}

// ---------- badge ----------

fn badge_of(b: &Badges, pane: &Value) -> Value {
    let Some(r) = b.refs.get(pane["paneId"].as_str().unwrap_or_default()) else { return Value::Null };
    if let Some(problem) = cli().problem_badge(&r.org, &PROBLEM_TEXT) {
        return problem;
    }
    let item = b.items.get(&r.key);
    let mut badge = badge_for(r, item);
    if !badge.is_null() {
        badge["loading"] = (b.manual.contains(&r.key) || (b.in_flight.contains(&r.key) && item.is_none())).into();
    }
    badge
}

fn render() {
    let badges: Vec<(String, Value)> = with(|b| b.panes.iter().map(|pane| (pane["paneId"].as_str().unwrap_or_default().to_owned(), badge_of(b, pane))).collect());
    for (pane, badge) in badges {
        spawn(async move { drop(host::set_badge(&pane, &badge).await) });
    }
}

/// A change of what the CLI can do shows in the badges, and Try again and signing in read again what failed for lack
/// of the CLI or a sign-in.
fn watch_cli() {
    cli().on_change(render);
    cli().on_retry(|| async {
        with(|b| b.items.clear());
        PARENT_TITLES.with(Cached::clone).clear();
        TYPE_STATES.with(Cached::clone).clear();
        refresh_refs(Which::All).await;
    });
}

/// The item tab moved a work item to another state.
fn item_changed(moved: &Value) {
    let key = format!("{} {}", moved["org"].as_str().unwrap_or_default(), moved["id"]);
    spawn(refresh_item(key, true));
}

/// A button of a badge was clicked.
async fn act(action: Value) -> Result<(), String> {
    let asked = action["actionId"].as_str().unwrap_or_default().to_owned();
    match asked.as_str() {
        "signin" => return cli().sign_in(None).await,
        "retry" => {
            cli().retry().await;
            return Ok(());
        }
        "install" => return cli().install().await,
        _ => {}
    }
    let Some(r) = with(|b| b.refs.get(action["paneId"].as_str().unwrap_or_default()).cloned()) else {
        return Ok(());
    };
    let found = with(|b| match b.items.get(&r.key) {
        Some(Item::Found(found)) => Some(found.clone()),
        _ => None,
    });
    match asked.as_str() {
        "refresh" => {
            refresh_item(r.key.clone(), true).await;
            Ok(())
        }
        "details" => host::open_view_with("item", "tab", &item_tab_args(r.id.parse().unwrap_or_default(), &r.org)).await.map(drop),
        "open" => match &found {
            Some(found) => host::call("openUrl", json!({ "url": found.url })).await.map(drop),
            None => Ok(()),
        },
        _ => match asked.strip_prefix("move:") {
            Some(state) => match &found {
                Some(found) if next_states(found).iter().any(|next| next == state) => move_to(&r, state).await,
                _ => Err(format!("{state} isn't a next state of this work item any more.")),
            },
            None => Ok(()),
        },
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    host::on_panes(|list| {
        with(|b| b.panes = list.to_vec());
        spawn(refresh_refs(Which::All));
    });
    // Wings says when a project's repo changes, so the branch is read then. Only panes in a repo that is no project's
    // keep a poll. Both pause while Wings is in the background, and the repo changes are held until you come back.
    on_change(|ids, _| refresh_refs(Which::Projects(ids)));
    poll::while_focused(BRANCH_POLL_MS, false, || spawn(refresh_refs(Which::Unwatched)));
    poll::while_focused(ITEM_POLL_MS, false, || {
        let keys: HashSet<String> = with(|b| b.refs.values().map(|r| r.key.clone()).collect());
        for key in keys {
            spawn(refresh_item(key, false));
        }
    });

    watch_cli();
    // The item tab moved a work item to another state.
    messages::on("item-changed", item_changed);

    host::on_action(act);
    project_sections::start();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use wings_plugin::task::block_on;

    const ORG: &str = "https://dev.azure.com/acme";

    #[derive(Default)]
    struct World {
        badges: HashMap<String, Value>,
        execs: Vec<Vec<String>>,
        signed_out: bool,
        login_fails: bool,
        update_rejects: bool,
        shown_while_signing_in: String,
    }

    /// A fake Wings with two panes, each on a branch of the Azure project `acme/Web`, and an `az` that answers for it.
    fn world() -> Rc<RefCell<World>> {
        let world: Rc<RefCell<World>> = Rc::default();
        let seen = world.clone();
        host::testing::answer(move |method, params| {
            let mut w = seen.borrow_mut();
            let ok = |stdout: Value| Ok(json!({ "code": 0, "stdout": if stdout.is_string() { stdout } else { stdout.to_string().into() }, "stderr": "" }));
            match method {
                "projects" => Ok(json!([])),
                "setBadge" => {
                    w.badges.insert(params["paneId"].as_str().unwrap().to_owned(), params["badge"].clone());
                    Ok(Value::Null)
                }
                "exec" => {
                    let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect();
                    w.execs.push(args.clone());
                    if params["program"] == "git" {
                        return match args[0].as_str() {
                            "remote" => ok(json!("git@ssh.dev.azure.com:v3/acme/Web/web")),
                            "rev-parse" => ok(params["cwd"].clone()),
                            _ => ok(json!(if params["cwd"] == "/a" { "feature/1111-a" } else { "feature/2222-b" })),
                        };
                    }
                    if args[0] == "login" {
                        // What the pane would be sent now: the badge task has not run yet while the login does.
                        w.shown_while_signing_in = with(|b| badge_of(b, &json!({ "paneId": "a" })))["label"].as_str().unwrap_or_default().to_owned();
                        return if w.login_fails { Ok(json!({ "code": 1, "stdout": "", "stderr": "ERROR: Login cancelled\n" })) } else { ok(json!("")) };
                    }
                    if w.signed_out {
                        return Ok(json!({ "code": 1, "stdout": "", "stderr": "ERROR: Please run 'az login' to setup account.\n" }));
                    }
                    if args[2] == "update" {
                        return if w.update_rejects { Err("az timed out after 30 s".into()) } else { ok(json!({})) };
                    }
                    if args[0] == "rest" {
                        let url = &args[args.iter().position(|a| a == "--url").unwrap() + 1];
                        let ids: Vec<u64> = url.split("ids=").nth(1).unwrap().split('&').next().unwrap().split(',').map(|id| id.parse().unwrap()).collect();
                        let value: Vec<Value> = ids.iter().map(|id| json!({ "id": id, "fields": { "System.Title": "T", "System.TeamProject": "Web", "System.WorkItemType": "Bug", "System.State": "Active" } })).collect();
                        return ok(json!({ "value": value }));
                    }
                    ok(json!({ "value": [{ "name": "Active", "category": "InProgress" }, { "name": "Resolved", "category": "Resolved" }] }))
                }
                _ => Ok(Value::Null),
            }
        });
        watch_cli();
        with(|b| b.panes = vec![json!({ "paneId": "a", "cwd": "/a" }), json!({ "paneId": "b", "cwd": "/b" })]);
        block_on(refresh_refs(Which::All));
        world
    }

    /// The ids of the work items read, as they come in `az rest` calls.
    fn shows(world: &Rc<RefCell<World>>) -> Vec<String> {
        world.borrow().execs.iter().filter(|e| e[0] == "rest").flat_map(|e| e[e.iter().position(|a| a == "--url").unwrap() + 1].split("ids=").nth(1).unwrap().split('&').next().unwrap().split(',').map(str::to_owned).collect::<Vec<_>>()).collect()
    }

    fn label(world: &Rc<RefCell<World>>, pane: &str) -> String {
        world.borrow().badges[pane]["label"].as_str().unwrap_or_default().to_owned()
    }

    #[test]
    fn the_panes_branches_read_in_one_az_call_and_each_pane_gets_its_badge() {
        let world = world();
        assert_eq!(shows(&world), ["1111", "2222"]);
        assert_eq!(world.borrow().execs.iter().filter(|e| e[0] == "rest").count(), 1);
        assert_eq!((label(&world, "a"), label(&world, "b")), ("#1111 Active".to_owned(), "#2222 Active".to_owned()));
    }

    #[test]
    fn item_changed_from_the_item_tab_refreshes_that_work_item_and_no_other() {
        let world = world();
        world.borrow_mut().execs.clear();
        item_changed(&json!({ "org": ORG, "id": 2222 }));
        block_on(async {});
        assert_eq!(shows(&world), ["2222"]);
    }

    #[test]
    fn a_failed_sign_in_puts_the_sign_in_button_back_instead_of_leaving_signing_in() {
        let world = world();
        world.borrow_mut().signed_out = true;
        block_on(act(json!({ "paneId": "a", "actionId": "refresh" }))).unwrap();
        assert_eq!(label(&world, "a"), "Sign in to Azure DevOps");
        world.borrow_mut().login_fails = true;
        assert_eq!(block_on(act(json!({ "paneId": "a", "actionId": "signin" }))), Err("ERROR: Login cancelled".into()));
        assert_eq!(world.borrow().shown_while_signing_in, "Signing in…");
        assert_eq!(label(&world, "a"), "Sign in to Azure DevOps");
        assert_eq!(world.borrow().badges["a"]["loading"], false);
        {
            let mut w = world.borrow_mut();
            (w.signed_out, w.login_fails) = (false, false);
        }
        block_on(act(json!({ "paneId": "a", "actionId": "retry" }))).unwrap();
        assert_eq!(label(&world, "a"), "#1111 Active");
    }

    #[test]
    fn a_move_whose_az_call_cant_run_stops_the_spinner() {
        let world = world();
        world.borrow_mut().update_rejects = true;
        assert_eq!(block_on(act(json!({ "paneId": "a", "actionId": "move:Resolved" }))), Err("az timed out after 30 s".into()));
        assert_eq!(world.borrow().badges["a"]["loading"], false);
    }

    #[test]
    fn a_move_to_something_that_isnt_a_next_state_is_refused_and_one_that_is_runs_az_and_reads_the_item_again() {
        let world = world();
        assert_eq!(block_on(act(json!({ "paneId": "a", "actionId": "move:Nowhere" }))), Err("Nowhere isn't a next state of this work item any more.".into()));
        world.borrow_mut().execs.clear();
        block_on(act(json!({ "paneId": "a", "actionId": "move:Resolved" }))).unwrap();
        let w = world.borrow();
        let update = w.execs.iter().find(|e| e.get(2).is_some_and(|word| word == "update")).unwrap();
        assert_eq!(&update[..7], ["boards", "work-item", "update", "--id", "1111", "--state", "Resolved"]);
        assert!(w.execs.iter().any(|e| e[0] == "rest"));
    }
}
