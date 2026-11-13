//! The Schedules tab: what is waiting for you, the saved entries with what each does next and last did, and the form to add
//! or change one. It reads the saved entries itself and asks main for every change.

use crate::{
    STATE, WANT,
    assigned::{AssignedTicket, DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, Project, Waiting, Watch, WatchDraft, WATCH_NAME, describe_check, started_elsewhere, state_options, title_wanted},
    page_ask::{ask, ask_quiet, busy, clear_error, error, listen},
    schedule::{Agent, DAYS, DAY_NUMBERS, Draft, EVERY_HOURS, Schedule, When, describe_next, describe_when, next_run},
    store::{read_schedules, read_watches},
    view::{busy_button, button, field, field_group, small_badge, textarea},
    worktree_setup::{WorktreeSetup, include_text, worktree_change},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    format::ago,
    git, host, messages, poll,
    task::spawn,
    ui::{Choice, Combobox, Node, card, chip, chips, combobox, confirm, disclosure, empty, heading, list, list_row, loading, muted, notice, page, row, select, slot, stack, text, text_input, title, toolbar, toolbar_with},
};

/// How often "in 2 h" is worked out again.
const CLOCK_MS: u32 = 30_000;
const REFS: [&str; 4] = ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"];

/// The ticket a notification's Start button asked for, and how starting it went.
struct Inbox {
    entry: String,
    ticket: String,
    status: InboxStatus,
}

enum InboxStatus {
    Starting,
    Started,
    Failed(String),
}

/// What the tab shows: the list, a new entry's form, or the form of the schedule or "assigned" entry with this id.
#[derive(Clone, PartialEq)]
enum Editing {
    List,
    New,
    Entry(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Schedule,
    Assigned,
}

/// What the form holds. Both kinds keep their fields here, so a new entry that tries the other kind and comes back finds what it typed.
struct Form {
    kind: Kind,
    name: String,
    project_id: String,
    agent: Agent,
    prompt: String,
    daily: bool,
    hours: i64,
    time: String,
    days: Vec<i64>,
    template: String,
    min_free: String,
    states: Option<Vec<String>>,
    prefixes: String,
    base: String,
    branches: Vec<String>,
    /// The project's worktree setting, loaded for the project picked.
    loaded: Option<(String, WorktreeSetup)>,
    include_field: String,
    setup_field: String,
    /// Goes up with each load: an older one that finds a later number drops out.
    loads: u64,
}

impl Form {
    fn new(schedule: Option<&Schedule>, watch: Option<&Watch>, projects: &[Project], here: Option<&str>) -> Self {
        let when = schedule.map(|s| &s.when);
        Form {
            kind: if watch.is_some() { Kind::Assigned } else { Kind::Schedule },
            name: schedule.map(|s| s.name.clone()).or_else(|| watch.and_then(|w| w.name.clone())).unwrap_or_default(),
            project_id: schedule.map(|s| s.project_id.clone()).or_else(|| watch.map(|w| w.project_id.clone())).or_else(|| here.map(str::to_owned)).or_else(|| projects.first().map(|p| p.id.clone())).unwrap_or_default(),
            agent: schedule.map(|s| s.agent).or_else(|| watch.map(|w| w.agent)).unwrap_or(Agent::Claude),
            prompt: schedule.map(|s| s.prompt.clone()).unwrap_or_default(),
            daily: !matches!(when, Some(When::Every { .. })),
            hours: if let Some(When::Every { hours }) = when { *hours } else { 24 },
            time: if let Some(When::Daily { time, .. }) = when { time.clone() } else { "09:00".into() },
            days: if let Some(When::Daily { days, .. }) = when { days.clone() } else { vec![1, 2, 3, 4, 5] },
            template: watch.map_or_else(|| DEFAULT_PROMPT.to_owned(), |w| w.prompt.clone()),
            min_free: watch.map_or(DEFAULT_MIN_FREE_GB, |w| w.min_free_gb).to_string(),
            states: watch.and_then(|w| w.states.clone()),
            prefixes: watch.and_then(|w| w.prefixes.clone()).unwrap_or_default().join(", "),
            base: watch.and_then(|w| w.base.clone()).unwrap_or_default(),
            branches: Vec::new(),
            loaded: None,
            include_field: String::new(),
            setup_field: String::new(),
            loads: 0,
        }
    }
}

struct Page {
    schedules: Vec<Schedule>,
    watches: Vec<Watch>,
    /// None until Wings has listed them.
    projects: Option<Vec<Project>>,
    /// Why the entries couldn't be read.
    load_error: Option<String>,
    /// The projects each entry's tickets can start in, from main.
    targets: HashMap<String, Vec<(String, String)>>,
    sign_in_code: Option<String>,
    /// The project the tab is in: the page shows its entries, until you ask for all.
    here: Option<String>,
    everywhere: bool,
    /// The entry a notification sent you here for: its waiting tickets come first.
    focus_watch: Option<String>,
    inbox: Option<Inbox>,
    editing: Editing,
    deleting: Option<String>,
    checking: Option<String>,
    /// The ticket whose Start menu is open: the entry's id and the ticket's.
    menu: Option<(String, String)>,
    form: Form,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page {
        schedules: Vec::new(), watches: Vec::new(), projects: None, load_error: None, targets: HashMap::new(), sign_in_code: None, here: None, everywhere: false,
        focus_watch: None, inbox: None, editing: Editing::List, deleting: None, checking: None, menu: None, form: Form::new(None, None, &[], None),
    });
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

fn name_of_project(projects: &[Project], id: &str) -> String {
    projects.iter().find(|p| p.id == id).map_or_else(|| "Missing project".to_owned(), |p| p.name.clone())
}

// ---------- loading ----------

/// Reads the saved entries again.
fn reload() {
    let read = read_schedules().and_then(|schedules| Ok((schedules, read_watches()?)));
    with(|p| match read {
        Ok((schedules, watches)) => (p.schedules, p.watches, p.load_error) = (schedules, watches, None),
        Err(error) => p.load_error = Some(error),
    });
}

/// What main told: the projects each entry can start in, and a sign-in that is running.
fn hear(state: &Value) {
    with(|p| {
        p.targets = state["targets"].as_object().map(|map| map.iter().map(|(id, list)| (id.clone(), list.as_array().map(|list| list.iter().map(|t| (t["id"].as_str().unwrap_or_default().to_owned(), t["name"].as_str().unwrap_or_default().to_owned())).collect()).unwrap_or_default())).collect()).unwrap_or_default();
        p.sign_in_code = state["signInCode"].as_str().map(str::to_owned);
    });
    reload();
}

/// Loads the branches and the worktree setting of the project the form is on, while it is an "assigned" entry's form.
fn load_form_data() {
    let Some((loads, project)) = with(|p| {
        p.form.loads += 1;
        p.form.loaded = None;
        (p.form.kind == Kind::Assigned && !p.form.project_id.is_empty()).then(|| (p.form.loads, p.projects.iter().flatten().find(|x| x.id == p.form.project_id).cloned().unwrap_or(Project { id: p.form.project_id.clone(), name: String::new(), path: String::new() })))
    }) else {
        return;
    };
    let id = project.id.clone();
    spawn(async move {
        let refs = if project.path.is_empty() { String::new() } else { git::ok(&project.path, &REFS).await.unwrap_or_default() };
        let branches: Vec<String> = refs.lines().filter(|b| !b.is_empty() && *b != "origin" && *b != "origin/HEAD").map(str::to_owned).collect();
        with(|p| {
            if p.form.loads == loads {
                p.form.branches = branches;
            }
        });
    });
    spawn(async move {
        let Ok(setup) = host::call("worktreeSetup", json!({ "projectId": id })).await else {
            return;
        };
        let setup = WorktreeSetup::from_json(&setup);
        with(|p| {
            if p.form.loads == loads {
                (p.form.include_field, p.form.setup_field) = (include_text(&setup), setup.setup.clone().unwrap_or_default());
                p.form.loaded = Some((id, setup));
            }
        });
    });
}

/// Opens the form on an entry, or on a new one, or goes back to the list, without the last action's error.
fn edit(editing: Editing) {
    clear_error();
    with(|p| {
        let schedule = if let Editing::Entry(id) = &editing { p.schedules.iter().find(|s| s.id == *id).cloned() } else { None };
        let watch = if let Editing::Entry(id) = &editing { p.watches.iter().find(|w| w.id == *id).cloned() } else { None };
        p.form = Form::new(schedule.as_ref(), watch.as_ref(), p.projects.as_deref().unwrap_or_default(), p.here.as_deref());
        p.editing = editing;
    });
    load_form_data();
}

fn go_back() {
    clear_error();
    with(|p| p.editing = Editing::List);
}

// ---------- the form ----------

fn save() {
    let sent = with(|p| {
        let f = &p.form;
        let editing = if let Editing::Entry(id) = &p.editing { Some(id.clone()) } else { None };
        if f.kind == Kind::Schedule {
            let when = if f.daily { When::Daily { time: f.time.clone(), days: f.days.clone() } } else { When::Every { hours: f.hours } };
            let draft = Draft { name: f.name.clone(), project_id: f.project_id.clone(), agent: f.agent, prompt: f.prompt.clone(), when };
            return ("save", json!({ "entry": editing, "draft": draft.to_json() }));
        }
        let moved = p.watches.iter().find(|w| Some(&w.id) == editing.as_ref()).is_some_and(|w| w.project_id != f.project_id);
        let min_free = f.min_free.trim().parse::<f64>().unwrap_or(if f.min_free.trim().is_empty() { 0.0 } else { f64::NAN });
        let draft = WatchDraft {
            name: Some(f.name.clone()),
            project_id: f.project_id.clone(),
            agent: f.agent,
            prompt: f.template.clone(),
            min_free_gb: min_free,
            // Another project has other states, so it picks its own at its first check.
            states: if moved { None } else { f.states.clone() },
            prefixes: f.prefixes.split(',').map(|prefix| prefix.trim().to_owned()).collect(),
            base: Some(f.base.clone()),
        };
        let mut args = json!({ "entry": editing, "draft": draft.to_json() });
        if let Some((project, setup)) = &f.loaded
            && let Some(change) = worktree_change(setup, &f.include_field, &f.setup_field)
        {
            args["worktree"] = json!({ "projectId": project, "change": change.to_json() });
        }
        ("saveWatch", args)
    });
    ask(sent.0, sent.1, go_back);
}

fn agent_chips(f: &Form) -> Node {
    chips("Agent", &[("claude", Agent::Claude.label()), ("opencode", Agent::OpenCode.label())], f.agent.id(), |value| with(|p| p.form.agent = Agent::from_id(value).unwrap_or(Agent::Claude)))
}

fn schedule_fields(f: &Form) -> Vec<Node> {
    let hours: Vec<(String, String)> = EVERY_HOURS.iter().map(|h| (h.to_string(), if *h == 1 { "1 hour".to_owned() } else { format!("{h} hours") })).collect();
    let when = if f.daily {
        row().prop("gap", "l").prop("wrap", true).prop("align", "start").child(field("Time", None, text_input("Time", &f.time, |time| with(|p| p.form.time = time)).prop("type", "time"))).child(field_group(
            "Days",
            None,
            row().prop("gap", "s").prop("wrap", true).children(DAYS.iter().zip(DAY_NUMBERS).map(|(label, number)| {
                chip(*label, f.days.contains(&number), move || {
                    with(|p| {
                        if let Some(at) = p.form.days.iter().position(|d| *d == number) {
                            p.form.days.remove(at);
                        } else {
                            p.form.days.push(number);
                        }
                    })
                })
                .key(*label)
            })),
        ))
    } else {
        row().child(field("Every", None, select("Every", &hours, &f.hours.to_string(), |hours| with(|p| p.form.hours = hours.parse().unwrap_or(24)))))
    };
    vec![
        field("Prompt", Some("What the agent is asked to do. It goes in as one line: line breaks become spaces.".into()), textarea("Prompt", &f.prompt, 4, false, |prompt| with(|p| p.form.prompt = prompt))),
        field_group(
            "When",
            None,
            stack().prop("gap", "m").child(chips("When", &[("daily", "At a time of day"), ("every", "Every few hours")], if f.daily { "daily" } else { "every" }, |mode| with(|p| p.form.daily = mode == "daily"))).child(when),
        ),
    ]
}

fn watch_fields(f: &Form, watch: Option<&Watch>) -> Vec<Node> {
    let state_names = watch.map_or_else(Vec::new, |w| state_options(f.states.as_deref(), w.tickets.as_deref()));
    let states_hint = if state_names.is_empty() {
        "The states show up after the first check. Wings then picks the ones that mean nobody has started: Open on GitHub and GitLab, and New, To Do, Ready for Development and the like on Azure DevOps."
    } else {
        "Tickets in other states are listed but not asked about. A ticket is asked about once, and one that moves into these states later is asked about then. Leave all off to ask about every state."
    };
    let states = row().prop("gap", "s").prop("wrap", true).children(state_names.iter().map(|name| {
        let toggled = name.clone();
        chip(name.as_str(), f.states.as_ref().is_some_and(|states| states.contains(name)), move || {
            with(|p| {
                let mut states = p.form.states.take().unwrap_or_default();
                if let Some(at) = states.iter().position(|s| *s == toggled) {
                    states.remove(at);
                } else {
                    states.push(toggled.clone());
                }
                p.form.states = Some(states);
            })
        })
        .key(name.clone())
    }));
    let include_hint = format!(
        "One pattern per line, .gitignore syntax: only files the repo ignores are copied, and a line starting with ! leaves files out. They are copied, not linked, so leave big folders out.{}",
        if f.loaded.as_ref().is_some_and(|(_, setup)| setup.has_include_file) { " This repo has a .worktreeinclude, so that file is used and this list is ignored." } else { "" }
    );
    let waiting = f.loaded.is_none();
    let base = combobox(
        Combobox { label: "Start branches from", placeholder: "The default branch", value: f.base.clone(), query: None, options: f.branches.iter().map(|b| Choice { value: b.clone(), label: b.clone(), detail: None }).collect(), empty: "No matches.", auto_focus: false },
        |typed| {
            if let Some(typed) = typed {
                with(|p| p.form.base = typed);
            }
        },
        |picked| with(|p| p.form.base = picked.to_owned()),
    );
    let mut include = Node::new("Textarea").prop("label", "Copy into new worktrees").prop("value", f.include_field.as_str()).prop("rows", 6).on("onChange", |event| with(|p| p.form.include_field = event["value"].as_str().unwrap_or_default().to_owned()));
    let mut setup = text_input("Setup command", &f.setup_field, |setup| with(|p| p.form.setup_field = setup)).prop("placeholder", "pnpm install");
    if waiting {
        include = include.prop("disabled", true);
        setup = setup.prop("disabled", true);
    }
    vec![
        field("Prompt", Some("Sent to the agent as one line. {id}, {title} and {url} become the ticket's number, title and link.".into()), textarea("Prompt", &f.template, 4, false, |template| with(|p| p.form.template = template))),
        field("Keep free", Some("In GB. Wings won't make the worktree when the disk it goes on has less room than this.".into()), text_input("Keep free", &f.min_free, |min_free| with(|p| p.form.min_free = min_free))),
        field_group("Ask about tickets in these states", Some(states_hint.into()), states),
        field("Start branches from", Some("Pick one of this project's branches or type one, like origin/develop. Empty starts from the repo's default branch.".into()), base),
        field(
            "Only titles starting with",
            Some("Optional, separated by commas, like FE or BE. Other tickets are listed as for another project and not asked about. Empty asks about every title.".into()),
            text_input("Only titles starting with", &f.prefixes, |prefixes| with(|p| p.form.prefixes = prefixes)).prop("placeholder", "FE, BE").prop("maxLength", 200),
        ),
        field_group(
            "New worktrees",
            Some("The project's own settings, shared with Worktree setup in the sidebar: a change here changes it there too.".into()),
            stack().prop("gap", "m").child(field("Copy into new worktrees", Some(include_hint), include)).child(field("Setup command", Some("One shell line, like pnpm install. It runs in the new worktree before the agent starts, and the agent starts once it succeeds.".into()), setup)),
        ),
        notice("info", "Wings asks the project's Azure DevOps, GitHub or GitLab what is assigned to you every 5 minutes, using its origin remote. You are asked first: nothing starts until you press Start."),
    ]
}

fn form_view(p: &Page) -> Node {
    let f = &p.form;
    let projects = p.projects.as_deref().unwrap_or_default();
    let (schedule, watch) = match &p.editing {
        Editing::Entry(id) => (p.schedules.iter().find(|s| s.id == *id), p.watches.iter().find(|w| w.id == *id)),
        _ => (None, None),
    };
    let existing = schedule.is_some() || watch.is_some();
    let options: Vec<(String, String)> = projects.iter().map(|x| (x.id.clone(), x.name.clone())).collect();
    let mut fields: Vec<Node> = Vec::new();
    if !existing {
        fields.push(field_group(
            "Start an agent",
            None,
            chips("Start an agent", &[("schedule", "On a schedule"), ("assigned", "When something is assigned to me")], if f.kind == Kind::Schedule { "schedule" } else { "assigned" }, |kind| {
                with(|p| p.form.kind = if kind == "assigned" { Kind::Assigned } else { Kind::Schedule });
                load_form_data();
            }),
        ));
    }
    fields.push(field(
        "Name",
        Some(if f.kind == Kind::Schedule { "Optional. Without one, the list shows the start of the prompt.".to_owned() } else { format!("Optional. Without one, the list says \"{WATCH_NAME}\".") }),
        text_input("Name", &f.name, |name| with(|p| p.form.name = name)).prop("maxLength", 80).prop("placeholder", if f.kind == Kind::Schedule { "Morning review" } else { "Frontend tickets" }),
    ));
    fields.push(
        row()
            .prop("gap", "l")
            .prop("wrap", true)
            .prop("align", "start")
            .child(field("Project", None, select("Project", &options, &f.project_id, |id| {
                with(|p| p.form.project_id = id.to_owned());
                load_form_data();
            })))
            .child(field("Agent", None, agent_chips(f))),
    );
    fields.extend(if f.kind == Kind::Schedule { schedule_fields(f) } else { watch_fields(f, watch) });
    if projects.is_empty() {
        fields.push(notice("warn", "Add a project in Wings first. An entry runs inside one."));
    }
    if let Some(message) = error() {
        fields.push(notice("danger", message));
    }
    fields.push(
        row()
            .prop("gap", "m")
            .prop("wrap", true)
            .child(busy_button(if existing { "Save" } else { "Add entry" }, "default", None, None, busy(), projects.is_empty(), save))
            .child(button("Cancel", "ghost", None, None, false, go_back)),
    );
    card().child(stack().prop("gap", "l").children(fields))
}

// ---------- the list ----------

/// A saved entry's card: its name, Paused while it is, its project, agent and `what` starts it, then `lines`, with
/// `actions` beside them. `below`, like a watch's tickets, goes under it all.
fn entry_card(key: String, enabled: bool, name: &str, project: &str, agent: Agent, what: &str, lines: Vec<Node>, actions: Vec<Node>, below: Option<Node>) -> Node {
    let head = row()
        .prop("gap", "l")
        .prop("justify", "between")
        .prop("wrap", true)
        .prop("grow", true)
        .child(
            stack()
                .prop("gap", "xs")
                .prop("grow", true)
                .child(row().prop("gap", "m").prop("wrap", true).child(title(name).prop("truncate", true)).maybe((!enabled).then(|| small_badge("Paused", None, None))))
                .child(muted(format!("{project} · {} · {what}", agent.label())).prop("size", "small"))
                .children(lines),
        )
        .child(row().prop("gap", "s").prop("wrap", true).prop("align", "start").children(actions));
    let content = match below {
        Some(below) => stack().prop("gap", "m").child(head).child(below),
        None => head,
    };
    list_row(key, card().child(content))
}

/// An entry's Edit, Pause or Resume, and Delete buttons, which name it `label` to a screen reader.
fn entry_actions(id: &str, enabled: bool, label: &str, working: bool, toggle: &'static str) -> Vec<Node> {
    let (edit_id, toggle_id, delete_id) = (id.to_owned(), id.to_owned(), id.to_owned());
    vec![
        button("Edit", "outline", Some("sm"), Some(format!("Edit {label}")), false, move || edit(Editing::Entry(edit_id.clone()))),
        button(if enabled { "Pause" } else { "Resume" }, "outline", Some("sm"), Some(format!("{} {label}", if enabled { "Pause" } else { "Resume" })), working, move || ask(toggle, json!({ "entry": toggle_id }), || {})),
        Node::new("Button")
            .prop("variant", "ghost")
            .prop("size", "icon-sm")
            .prop("label", format!("Delete {label}"))
            .on("onClick", move |_| with(|p| p.deleting = Some(delete_id.clone())))
            .child(Node::new("Icon").prop("name", "trash")),
    ]
}

fn ago_text(prefix: &str, at: i64, now: i64) -> Node {
    muted(format!("{prefix}{}", ago(at as f64, now as f64))).prop("size", "small")
}

fn last_run(run: Option<&crate::schedule::Run>, now: i64, prefix: &str) -> Node {
    let Some(run) = run else {
        return muted("Not run yet").prop("size", "small");
    };
    row()
        .prop("gap", "s")
        .prop("wrap", true)
        .child(ago_text(prefix, run.at, now))
        .child(if run.ok { small_badge("Started", Some("success"), None) } else { small_badge("Failed", Some("danger"), None) })
        .maybe((!run.ok).then(|| text(run.error.clone().unwrap_or_default()).prop("tone", "danger").prop("size", "small")))
}

fn schedule_item(s: &Schedule, project: &str, now: i64, working: bool) -> Node {
    let name = s.title();
    let next = next_run(s);
    let run_id = s.id.clone();
    let mut actions = vec![button("Run now", "outline", Some("sm"), Some(format!("Run {name} now")), working, move || ask("runNow", json!({ "entry": run_id }), || {}))];
    actions.extend(entry_actions(&s.id, s.enabled, &name, working, "toggle"));
    let when = if !s.enabled {
        "Next run when resumed".to_owned()
    } else {
        next.map_or_else(|| "No day picked".to_owned(), |at| format!("Next run {}", describe_next(at, now)))
    };
    entry_card(s.id.clone(), s.enabled, &name, project, s.agent, &describe_when(&s.when), vec![text(when).prop("size", "small"), last_run(s.last_run.as_ref(), now, "Last run ")], actions, None)
}

fn ticket_row(w: &Watch, t: &AssignedTicket, p: &Page, working: bool, now: i64) -> Node {
    let started_at = w.started.iter().find(|(id, _)| *id == t.ticket.id).map(|(_, at)| *at);
    let asked = w.asks(t);
    let elsewhere = started_elsewhere(t);
    let label = if started_at.is_some() { "Start again" } else if asked && elsewhere.is_none() { "Start" } else { "Start anyway" };
    let variant = if label == "Start" { "default" } else { "outline" };
    let force = !asked || elsewhere.is_some() || started_at.is_some();
    let start = {
        let (entry, ticket) = (w.id.clone(), t.ticket.id.clone());
        move |project: Option<String>| ask("startTicket", json!({ "entry": entry, "ticket": ticket, "force": force, "projectId": project }), || {})
    };
    let targets = p.targets.get(&w.id).cloned().unwrap_or_default();
    let title_ok = title_wanted(w.prefixes.as_deref(), &t.ticket.title);
    let url = t.ticket.url.clone();
    let start_control = if targets.len() > 1 {
        let open = p.menu.as_ref() == Some(&(w.id.clone(), t.ticket.id.clone()));
        let key = (w.id.clone(), t.ticket.id.clone());
        let mut menu = stack().prop("gap", "xs").child(button(label, variant, Some("sm"), Some(format!("{label} {}", t.ticket.title)), working, move || with(|p| p.menu = if open { None } else { Some(key.clone()) })));
        if open {
            menu = menu.children(targets.iter().map(|(id, name)| {
                let (start, project, key) = (start.clone(), id.clone(), id.clone());
                button(&format!("In {name}"), "ghost", None, None, false, move || {
                    with(|p| p.menu = None);
                    start(Some(project.clone()));
                })
                .key(key)
            }));
        }
        menu
    } else {
        button(label, variant, Some("sm"), Some(format!("{label} {}", t.ticket.title)), working, move || start(None))
    };
    let badge = if asked { small_badge(&t.state, None, Some("secondary")) } else { small_badge(&if title_ok { format!("Not asked: {}", t.state) } else { "Other project".to_owned() }, None, Some("outline")) };
    list_row(
        t.ticket.id.clone(),
        row()
            .prop("gap", "m")
            .prop("justify", "between")
            .prop("wrap", true)
            .prop("grow", true)
            .child(
                stack()
                    .prop("gap", "xs")
                    .prop("grow", true)
                    .child(Node::new("Text").prop("tone", if asked { "default" } else { "muted" }).prop("truncate", true).text(t.ticket.title.as_str()))
                    .child(muted(format!("#{}{}", t.ticket.id, t.kind.as_ref().map_or(String::new(), |kind| format!(" · {kind}")))).prop("size", "small"))
                    .maybe(elsewhere.map(|started| muted(format!("Started: {started}")).prop("size", "small").prop("truncate", true))),
            )
            .child(
                row()
                    .prop("gap", "s")
                    .prop("wrap", true)
                    .prop("align", "start")
                    .child(badge)
                    .child(button("Open", "ghost", Some("sm"), Some(format!("Open {}", t.ticket.title)), false, move || host::open_url(&url)))
                    .maybe(started_at.map(|at| ago_text("Started ", at, now)))
                    .child(start_control),
            ),
    )
}

fn watch_item(w: &Watch, p: &Page, now: i64, working: bool) -> Node {
    let project = name_of_project(p.projects.as_deref().unwrap_or_default(), &w.project_id);
    let check = w.last_check.as_ref();
    let label = format!("{} in {project}", w.title());
    let unsupported = w.source == Some(crate::assigned::Source::Unsupported);
    let failed = check.is_some_and(|c| !c.ok);
    let mut actions: Vec<Node> = Vec::new();
    if w.enabled && check.and_then(|c| c.problem) == Some(crate::assigned::Problem::SignedOut) {
        let id = w.id.clone();
        actions.push(button("Sign in", "default", Some("sm"), Some(format!("Sign in for {label}")), working, move || ask("signIn", json!({ "entry": id }), || {})));
    }
    if w.enabled && check.and_then(|c| c.problem) == Some(crate::assigned::Problem::Missing) {
        let id = w.id.clone();
        actions.push(button("Install", "default", Some("sm"), Some(format!("Install the CLI for {label}")), working, move || ask("install", json!({ "entry": id }), || {})));
    }
    if w.enabled && !unsupported {
        let id = w.id.clone();
        actions.push(busy_button(
            if failed { "Try again" } else { "Check now" },
            "outline",
            Some("sm"),
            Some(format!("{} {label} {}", if failed { "Try" } else { "Check" }, if failed { "again" } else { "now" })),
            p.checking.as_deref() == Some(w.id.as_str()),
            working,
            move || {
                with(|p| p.checking = Some(id.clone()));
                ask("checkWatch", json!({ "entry": id }), || with(|p| p.checking = None));
            },
        ));
    }
    actions.extend(entry_actions(&w.id, w.enabled, &label, working, "toggleWatch"));
    let tickets = w.tickets.as_deref().unwrap_or_default();
    let below = (!tickets.is_empty()).then(|| disclosure(format!("Assigned to you ({})", tickets.len()), list().children(tickets.iter().map(|t| ticket_row(w, t, p, working, now)))));
    let mut lines: Vec<Node> = Vec::new();
    if w.enabled {
        lines.push(
            row()
                .prop("gap", "s")
                .prop("wrap", true)
                .maybe(check.filter(|_| !unsupported).map(|c| ago_text("Checked ", c.at, now)))
                .child(text(describe_check(check, w.source.as_ref())).prop("size", "small").prop("tone", if failed { "danger" } else { "default" })),
        );
    } else {
        lines.push(text("Not looking while paused").prop("size", "small"));
    }
    if !w.waiting.is_empty() {
        lines.push(text(if w.waiting.len() == 1 { "1 ticket is waiting for you".to_owned() } else { format!("{} tickets are waiting for you", w.waiting.len()) }).prop("size", "small"));
    }
    if w.last_run.is_some() {
        lines.push(last_run(w.last_run.as_ref(), now, "Last started "));
    }
    entry_card(w.id.clone(), w.enabled, w.title(), &project, w.agent, w.source.as_ref().map_or("Checking the remote", |source| source.label()), lines, actions, below)
}

fn waiting_item(w: &Watch, ticket: &Waiting, project: &str, working: bool) -> Node {
    let (start, dismiss) = ((w.id.clone(), ticket.ticket.id.clone()), (w.id.clone(), ticket.ticket.id.clone()));
    let url = ticket.ticket.url.clone();
    list_row(
        format!("{}:{}", w.id, ticket.ticket.id),
        card().child(
            row()
                .prop("gap", "l")
                .prop("justify", "between")
                .prop("wrap", true)
                .prop("grow", true)
                .child(
                    stack()
                        .prop("gap", "xs")
                        .prop("grow", true)
                        .child(title(ticket.ticket.title.as_str()).prop("truncate", true))
                        .child(muted(format!("#{} · {project} · {}", ticket.ticket.id, w.agent.label())).prop("size", "small"))
                        .child(text("Starts a new worktree on a feature branch and opens the agent in it.").prop("size", "small")),
                )
                .child(
                    row()
                        .prop("gap", "s")
                        .prop("wrap", true)
                        .prop("align", "start")
                        .child(button("Open", "ghost", Some("sm"), Some(format!("Open {}", ticket.ticket.title)), working, move || host::open_url(&url)))
                        .child(busy_button("Start", "default", Some("sm"), Some(format!("Start {}", ticket.ticket.title)), working, false, move || ask("startTicket", json!({ "entry": start.0, "ticket": start.1 }), || {})))
                        .child(button("Dismiss", "outline", Some("sm"), Some(format!("Dismiss {}", ticket.ticket.title)), working, move || ask("dismissWaiting", json!({ "entry": dismiss.0, "ticket": dismiss.1 }), || {}))),
                ),
        ),
    )
}

/// A section's heading, in a list of its own so it starts where the rows below it do.
fn section_heading(label: &str) -> Node {
    list().child(list_row("heading", heading(3, label)))
}

/// A ticket's title from what the entry waits on or listed, else its number.
fn ticket_title(w: Option<&Watch>, id: &str) -> String {
    w.and_then(|w| w.waiting.iter().find(|t| t.ticket.id == id).map(|t| t.ticket.title.clone()).or_else(|| w.tickets.iter().flatten().find(|t| t.ticket.id == id).map(|t| t.ticket.title.clone()))).unwrap_or_else(|| format!("#{id}"))
}

fn list_view(p: &Page, now: i64) -> Node {
    let projects = p.projects.as_deref().unwrap_or_default();
    let working = busy();
    let in_scope = |project_id: &str, id: &str| p.everywhere || p.here.is_none() || p.here.as_deref() == Some(project_id) || p.focus_watch.as_deref() == Some(id);
    let schedules: Vec<&Schedule> = p.schedules.iter().filter(|s| in_scope(&s.project_id, &s.id)).collect();
    let watches: Vec<&Watch> = p.watches.iter().filter(|w| in_scope(&w.project_id, &w.id)).collect();
    let elsewhere = p.schedules.len() + p.watches.len() - schedules.len() - watches.len();
    let empty_list = schedules.is_empty() && watches.is_empty();
    let mut waiting: Vec<(&Watch, &Waiting)> = watches.iter().flat_map(|w| w.waiting.iter().map(move |t| (*w, t))).collect();
    waiting.sort_by_key(|(w, _)| p.focus_watch.as_deref() != Some(w.id.as_str()));

    let mut body: Vec<Node> = Vec::new();
    if let Some(message) = error() {
        body.push(notice("danger", message));
    }
    if let Some(message) = &p.load_error {
        body.push(notice("warn", message.clone()));
    }
    if let Some(inbox) = &p.inbox {
        let (tone, said) = match &inbox.status {
            InboxStatus::Failed(error) => ("danger", error.clone()),
            InboxStatus::Started => ("success", format!("Started {}.", ticket_title(watches.iter().find(|w| w.id == inbox.entry).copied(), &inbox.ticket))),
            InboxStatus::Starting => ("info", format!("Starting {}.", ticket_title(watches.iter().find(|w| w.id == inbox.entry).copied(), &inbox.ticket))),
        };
        body.push(notice(tone, said));
    }
    if let Some(code) = &p.sign_in_code {
        body.push(notice("info", format!("Your GitHub code is {code}. It is on your clipboard: paste it in the browser to finish signing in.")));
    }
    if let Some(id) = &p.deleting {
        let doomed_schedule = schedules.iter().find(|s| s.id == *id);
        if doomed_schedule.is_some() || watches.iter().any(|w| w.id == *id) {
            let delete = id.clone();
            let op = if doomed_schedule.is_some() { "remove" } else { "removeWatch" };
            body.push(confirm(
                format!("Delete {}?", doomed_schedule.map_or("this entry".to_owned(), |s| s.title())),
                [text(if doomed_schedule.is_some() { "It won't run again. Tabs it already started stay open." } else { "It stops watching, and what is waiting for you goes with it. Tabs it already started stay open." })],
                [
                    button("Delete", "destructive", None, None, working, move || ask(op, json!({ "entry": delete }), || with(|p| p.deleting = None))),
                    button("Cancel", "ghost", None, None, working, || with(|p| p.deleting = None)),
                ],
            ));
        }
    }
    if !waiting.is_empty() {
        body.push(
            stack()
                .prop("gap", "m")
                .child(section_heading("Waiting for you"))
                .child(list().children(waiting.iter().map(|(w, ticket)| waiting_item(w, ticket, &name_of_project(projects, &w.project_id), working)))),
        );
    }
    if empty_list {
        body.push(
            empty(
                if elsewhere > 0 { "Nothing set up for this project" } else { "Nothing set up yet" },
                "Pick a project and a prompt, and Wings starts the agent for you at the time you choose, or when a ticket is assigned to you. Wings has to be running.",
            )
            .child(slot("children", button("Add entry", "default", None, None, false, || edit(Editing::New)))),
        );
    } else {
        body.push(
            stack()
                .prop("gap", "m")
                .maybe((!waiting.is_empty()).then(|| section_heading("Entries")))
                .child(list().children(schedules.iter().map(|s| schedule_item(s, &name_of_project(projects, &s.project_id), now, working)).chain(watches.iter().map(|w| watch_item(w, p, now, working))))),
        );
    }
    stack().prop("gap", "l").children(body)
}

fn toolbar_actions(p: &Page) -> Option<Node> {
    let projects_here = p.here.as_deref();
    let in_scope = |project_id: &str, id: &str| p.everywhere || projects_here.is_none() || projects_here == Some(project_id) || p.focus_watch.as_deref() == Some(id);
    let shown = p.schedules.iter().filter(|s| in_scope(&s.project_id, &s.id)).count() + p.watches.iter().filter(|w| in_scope(&w.project_id, &w.id)).count();
    let elsewhere = p.schedules.len() + p.watches.len() - shown;
    let scope = (projects_here.is_some() && (p.everywhere || elsewhere > 0)).then(|| {
        let everywhere = p.everywhere;
        button(&if everywhere { "This project".to_owned() } else { format!("All projects ({elsewhere} more)") }, "ghost", None, None, false, move || with(|p| p.everywhere = !everywhere))
    });
    (shown > 0 || scope.is_some()).then(|| row().prop("gap", "s").maybe(scope).maybe((shown > 0).then(|| button("Add entry", "default", None, None, false, || edit(Editing::New)))))
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let now = host::now() as i64;
        if p.projects.is_none() {
            return vec![match &p.load_error {
                Some(error) => page(toolbar("Schedules", None), vec![empty("Couldn't load your schedules", error)]),
                None => page(toolbar("Schedules", None), vec![loading().text("Loading schedules")]),
            }];
        }
        if p.editing != Editing::List {
            let name = match &p.editing {
                Editing::Entry(id) => p.schedules.iter().find(|s| s.id == *id).map(|s| s.title()).or_else(|| p.watches.iter().find(|w| w.id == *id).map(|w| w.title().to_owned())).unwrap_or_else(|| "New entry".to_owned()),
                _ => "New entry".to_owned(),
            };
            let back = row().prop("gap", "s").child(button("← Schedules", "ghost", None, None, false, go_back)).child(muted(name));
            return vec![page(toolbar_with("Schedules", back, None), vec![stack().prop("maxWidth", 720).child(form_view(p))])];
        }
        vec![page(toolbar("Schedules", toolbar_actions(p)), vec![list_view(p, now)])]
    })
}

// ---------- starting ----------

/// What a notification's buttons opened the tab with: the entry to show first, and a ticket to start.
fn open_with(args: Option<Value>) {
    let args = args.unwrap_or(Value::Null);
    let waiting = args["waiting"].as_str().map(str::to_owned);
    let start = args["start"]["entry"].as_str().zip(args["start"]["ticket"].as_str()).map(|(entry, ticket)| (entry.to_owned(), ticket.to_owned()));
    with(|p| p.focus_watch = waiting.or_else(|| start.as_ref().map(|(entry, _)| entry.clone())));
    if let Some((entry, ticket)) = start {
        with(|p| p.inbox = Some(Inbox { entry: entry.clone(), ticket: ticket.clone(), status: InboxStatus::Starting }));
        let (e, t) = (entry.clone(), ticket.clone());
        ask_quiet("startTicket", json!({ "entry": entry, "ticket": ticket }), move |result| {
            with(|p| p.inbox = Some(Inbox { entry: e, ticket: t, status: match result { Ok(()) => InboxStatus::Started, Err(error) => InboxStatus::Failed(error) } }))
        });
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run_filled(view);
    listen("s");
    reload();
    messages::on(STATE, hear);
    host::on_projects(|list| {
        with(|p| p.projects = Some(Project::list(&Value::Array(list.to_vec()))));
        // A form that was waiting for its project's branches finds them.
        if with(|p| p.editing != Editing::List && p.form.loaded.is_none() && p.form.branches.is_empty()) {
            load_form_data();
        }
    });
    host::on("project", |_, project| with(|p| p.here = project["id"].as_str().map(str::to_owned)));
    // "in 2 h" is worked out again each time the page is built, which this makes happen.
    poll::while_focused(CLOCK_MS, false, || {});
    open_with(host::view_args());
    messages::tell(WANT, &Value::Null);
}
