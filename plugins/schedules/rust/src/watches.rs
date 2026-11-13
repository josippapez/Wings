//! The "When something is assigned to me" entries: one CLI call per entry every few minutes, a notification for each ticket
//! that is new, and Start, which makes the worktree and opens the agent in it.

use crate::{
    assigned::{
        AssignedTicket, Branch, Check, Disk, Issue, Problem, Project, Source, Ticket, UNSUPPORTED, Waiting, Watch, WatchDraft, apply_check, branch_matching, branch_name, clean_watch_draft, created_key, disk_problem, fill_prompt,
        issue_description, issue_line, issue_type_of, key_of, list_command, mentions_ticket, open_issue_keys, parse_tickets, same_source, source_of, started_elsewhere, with_issue_key, with_started,
    },
    forge::Program,
    main_frame::{changed, new_id, write_watches},
    schedule::Run,
    store::read_watches,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use wings_plugin::{
    cli::{CliError, Kind, last_line},
    git, host,
    task::{SingleFlight, map_limit, spawn},
};

/// A check is one CLI call, so it runs while Wings is in the background too: that is when a new ticket is wanted.
pub const CHECK_MS: u32 = 5 * 60_000;
/// An Issues list line for an issue that was started or finished: its key and status.
const STARTED: [&str; 3] = ["in_progress", "in_review", "done"];
/// The most issues a search finds that are read for one ticket in one project: a short id like 12 finds many.
const ISSUES_READ: usize = 5;
const REFS: [&str; 4] = ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"];

thread_local! {
    /// The remote of each project, read once: a check is then the one call. Forgotten on Try again.
    static SOURCES: RefCell<HashMap<String, Source>> = RefCell::new(HashMap::new());
    static STARTING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static SIGN_IN_CODE: RefCell<Option<String>> = const { RefCell::new(None) };
    static CHECK_ALL: SingleFlight = SingleFlight::new(|| async {
        for w in read_watches().unwrap_or_default().into_iter().filter(|w| w.enabled) {
            check_one(&w.id).await;
        }
    });
}

pub fn forget_sources() {
    SOURCES.with(|sources| sources.borrow_mut().clear());
}

pub fn sign_in_code() -> Option<String> {
    SIGN_IN_CODE.with(|code| code.borrow().clone())
}

pub async fn source_for(project: &Project) -> Source {
    let key = format!("{}\n{}", project.id, project.path);
    if let Some(source) = SOURCES.with(|sources| sources.borrow().get(&key).cloned()) {
        return source;
    }
    let source = source_of(git::ok(&project.path, &["remote", "get-url", "origin"]).await.as_deref());
    SOURCES.with(|sources| sources.borrow_mut().insert(key, source.clone()));
    source
}

/// The projects an entry's tickets can start in: the ones with its source, its own first, without the worktrees Wings made (`<repo>-worktrees/<branch>`).
pub async fn targets_of(w: &Watch, projects: &[Project]) -> Vec<Project> {
    let mut found: Vec<Project> = projects.iter().filter(|p| p.id == w.project_id).cloned().collect();
    if let Some(source) = &w.source {
        for p in projects {
            if p.id != w.project_id && !p.path.contains("-worktrees/") && same_source(&source_for(p).await, source) {
                found.push(p.clone());
            }
        }
    }
    found
}

/// The tickets, with the branch of each that has one in a project of the same source: one for-each-ref in each such project.
async fn mark_branches(source: &Source, tickets: Vec<AssignedTicket>, projects: &[Project], first_id: &str) -> Vec<AssignedTicket> {
    let mut sorted = projects.to_vec();
    sorted.sort_by_key(|p| p.id != first_id);
    let wanted = source.clone();
    let found = map_limit(sorted, 4, move |p| {
        let wanted = wanted.clone();
        async move {
            if !same_source(&source_for(&p).await, &wanted) {
                return None;
            }
            let refs = git::ok(&p.path, &REFS).await.unwrap_or_default();
            Some((p.name, refs.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect::<Vec<_>>()))
        }
    })
    .await;
    let sets: Vec<(String, Vec<String>)> = found.into_iter().flatten().collect();
    tickets
        .into_iter()
        .map(|t| {
            let branch = sets.iter().find_map(|(project, branches)| branch_matching(&t.ticket.id, branches).map(|name| Branch { name: name.clone(), project: project.clone() }));
            AssignedTicket { branch, ..t }
        })
        .collect()
}

/// What another plugin's tool answers, or nothing when it fails or is off.
async fn ask(name: &str, args: Value, project: &str) -> String {
    match host::call("callTool", json!({ "name": name, "arguments": args, "project": project })).await {
        Ok(answer) if answer["isError"] != true => answer["text"].as_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// The tickets without a branch, with a Wings issue that names one, in a project of the same source: an Issues search
/// for the id, then a read of each issue it finds, since the search also finds 12 in WIN-120. Nothing when Issues is off.
async fn mark_issues(tickets: Vec<AssignedTicket>, projects: Vec<Project>) -> Vec<AssignedTicket> {
    map_limit(tickets, 4, move |t| {
        let projects = projects.clone();
        async move {
            if t.branch.is_some() {
                return t;
            }
            for p in &projects {
                let listed = ask("tasks__list", json!({ "query": t.ticket.id }), &p.id).await;
                for (key, status) in listed.lines().filter_map(|line| issue_line(line, &STARTED)).take(ISSUES_READ) {
                    if mentions_ticket(&t.ticket.id, &ask("tasks__get", json!({ "key": key }), &p.id).await) {
                        return AssignedTicket { issue: Some(Issue { key, status, project: p.name.clone() }), ..t };
                    }
                }
            }
            t
        }
    })
    .await
}

/// A tool of Issues, which fails with the last line of what it said.
async fn call(name: &str, args: Value, project: &str) -> Result<String, String> {
    let answer = host::call("callTool", json!({ "name": name, "arguments": args, "project": project })).await?;
    let text = answer["text"].as_str().unwrap_or_default();
    if answer["isError"] == true {
        let line = last_line(text);
        return Err(if line.is_empty() { format!("{name} failed") } else { line.to_owned() });
    }
    Ok(text.to_owned())
}

/// Opens the Wings issue for a ticket that is being started, in the project the ticket starts in (not the worktree, whose
/// issues are kept there anyway): an open issue that names the ticket is set in progress on the branch, else one is created.
/// Never fails: Issues being off, or any of its calls failing, comes back as the problem and the agent starts without a key.
async fn track_issue(project_id: &str, ticket: &AssignedTicket, branch: &str) -> (Option<String>, Option<String>) {
    let tracked = async {
        let mut key: Option<String> = None;
        for k in open_issue_keys(&call("tasks__list", json!({ "query": ticket.ticket.id }), project_id).await?).into_iter().take(ISSUES_READ) {
            if mentions_ticket(&ticket.ticket.id, &call("tasks__get", json!({ "key": k }), project_id).await?) {
                key = Some(k);
                break;
            }
        }
        if let Some(key) = &key {
            call("tasks__update", json!({ "key": key, "status": "in_progress", "branch": branch }), project_id).await?;
            return Ok(key.clone());
        }
        let made = call(
            "tasks__create",
            json!({ "title": ticket.ticket.title, "type": issue_type_of(ticket.kind.as_deref()), "status": "in_progress", "description": issue_description(&ticket.ticket) }),
            project_id,
        )
        .await?;
        let key = created_key(&made).ok_or("Issues didn't say which key it made.")?;
        call("tasks__update", json!({ "key": key, "branch": branch }), project_id).await?;
        Ok::<String, String>(key)
    };
    match tracked.await {
        Ok(key) => (Some(key), None),
        Err(error) => {
            let line = last_line(&error);
            (None, Some(format!("Couldn't open a Wings issue ({})", if line.is_empty() { "Issues is off" } else { line })))
        }
    }
}

fn problem_of(error: &CliError, source: &Source, program: Program) -> (String, Option<Problem>) {
    match error.kind {
        Kind::Missing => (format!("The {} command isn't installed.", program.name()), Some(Problem::Missing)),
        Kind::SignedOut => (format!("Sign in to {} to see what is assigned to you.", source.label()), Some(Problem::SignedOut)),
        Kind::Failed => {
            let line = last_line(&error.message);
            (if line.is_empty() { "The check failed.".into() } else { line.to_owned() }, None)
        }
    }
}

/// Looks for what is assigned to you in one entry's project and records what it found.
pub async fn check_one(id: &str) {
    let Some(w) = read_watches().ok().and_then(|list| list.into_iter().find(|w| w.id == id)) else {
        return;
    };
    let Ok(listed) = host::projects().await else {
        return;
    };
    let projects = Project::list(&listed);
    let project = projects.iter().find(|p| p.id == w.project_id);
    let at = host::now() as i64;
    let mut source = w.source.clone();
    let mut tickets: Option<Vec<AssignedTicket>> = None;
    let check = match project {
        None => Check::failed(at, "The project is no longer in Wings.", None),
        Some(project) => {
            let found = source_for(project).await;
            source = Some(found.clone());
            match list_command(&found) {
                None => Check::failed(at, UNSUPPORTED, None),
                Some((name, args)) => {
                    let program = Program::of(name).unwrap_or(Program::Az);
                    let listed = match program.json(&args, key_of(&found).as_deref(), &project.path).await {
                        Ok(json) => parse_tickets(&found, &json).map_err(|message| CliError { kind: Kind::Failed, message }),
                        Err(error) => Err(error),
                    };
                    match listed {
                        Ok(list) => {
                            let list = mark_branches(&found, list, &projects, &project.id).await;
                            let mut same: Vec<Project> = Vec::new();
                            for p in &projects {
                                if same_source(&source_for(p).await, &found) {
                                    same.push(p.clone());
                                }
                            }
                            let list = mark_issues(list, same).await;
                            let check = Check { at, ok: true, count: Some(list.len() as i64), error: None, problem: None };
                            tickets = Some(list);
                            check
                        }
                        Err(error) => {
                            let (message, problem) = problem_of(&error, &found, program);
                            Check::failed(at, message, problem)
                        }
                    }
                }
            }
        }
    };
    let Ok(list) = read_watches() else {
        return;
    };
    let Some(current) = list.iter().find(|x| x.id == id).cloned() else {
        return;
    };
    let mut next = Watch { source, last_check: Some(check), ..current.clone() };
    let mut fresh: Vec<Waiting> = Vec::new();
    // Paused while the call ran: what it found is not looked at.
    if let (Some(found), true) = (&tickets, current.enabled) {
        (next, fresh) = apply_check(&next, found, at);
    }
    if write_watches(&list.into_iter().map(|x| if x.id == id { next.clone() } else { x }).collect::<Vec<_>>()).is_err() {
        return;
    }
    for t in fresh {
        let mut note = json!({
            "title": format!("Assigned: {}", t.ticket.title),
            "body": format!("{} · Start {} on it in a new worktree?", project.map_or("A project", |p| p.name.as_str()), w.agent.label()),
            "actions": [
                { "label": "Start", "open": { "view": "schedules", "args": { "start": { "entry": id, "ticket": t.ticket.id } } } },
                { "label": "Review", "open": { "view": "schedules", "args": { "waiting": id } } },
            ],
        });
        if let Some(project) = project {
            note["projectId"] = project.id.clone().into();
        }
        drop(host::call("notify", note));
    }
}

/// One call for each enabled entry.
pub async fn check_all() {
    CHECK_ALL.with(SingleFlight::call).await;
}

fn update(id: &str, change: impl Fn(Watch) -> Watch) -> Result<(), String> {
    let list = read_watches()?;
    if list.iter().any(|w| w.id == id) {
        write_watches(&list.into_iter().map(|w| if w.id == id { change(w) } else { w }).collect::<Vec<_>>())?;
    }
    Ok(())
}

fn find(id: &str) -> Result<Watch, String> {
    read_watches()?.into_iter().find(|w| w.id == id).ok_or_else(|| "That entry is gone.".into())
}

/// Adds an entry that has checked nothing yet. `only_one` refuses a project that already has one.
pub fn add_watch(draft: &WatchDraft, only_one: bool) -> Result<Watch, String> {
    let clean = clean_watch_draft(draft)?;
    let list = read_watches()?;
    if only_one && list.iter().any(|w| w.project_id == clean.project_id) {
        return Err("That project already has an assigned entry. Remove it first, or change it on the Schedules page.".into());
    }
    forget_sources();
    let saved = Watch {
        id: new_id(host::now() as i64),
        name: clean.name,
        project_id: clean.project_id,
        agent: clean.agent,
        prompt: clean.prompt,
        min_free_gb: clean.min_free_gb as i64,
        enabled: true,
        primed: false,
        seen: Vec::new(),
        waiting: Vec::new(),
        states: clean.states,
        prefixes: Some(clean.prefixes),
        base: clean.base,
        agents_start: clean.agents_start,
        started: Vec::new(),
        tickets: None,
        source: None,
        last_check: None,
        last_run: None,
    };
    write_watches(&[list, vec![saved.clone()]].concat())?;
    Ok(saved)
}

pub fn remove_watch(id: &str) -> Result<(), String> {
    write_watches(&read_watches()?.into_iter().filter(|w| w.id != id).collect::<Vec<_>>())
}

/// Changes an entry. One moved to another project starts over there, like a new one.
pub fn save_watch(id: &str, draft: &WatchDraft) -> Result<Watch, String> {
    let clean = clean_watch_draft(draft)?;
    let list = read_watches()?;
    forget_sources();
    let old = list.iter().find(|w| w.id == id).cloned().ok_or("That entry is gone.")?;
    // Another project has other tickets, so it starts over: nothing it already has is asked about.
    let moved = old.project_id != clean.project_id;
    let mut saved = Watch {
        name: clean.name,
        project_id: clean.project_id,
        agent: clean.agent,
        prompt: clean.prompt,
        min_free_gb: clean.min_free_gb as i64,
        states: clean.states,
        prefixes: Some(clean.prefixes),
        base: clean.base,
        agents_start: clean.agents_start,
        ..old
    };
    if moved {
        saved = Watch { primed: false, seen: Vec::new(), waiting: Vec::new(), tickets: None, source: None, last_check: None, ..saved };
    }
    write_watches(&list.into_iter().map(|w| if w.id == id { saved.clone() } else { w }).collect::<Vec<_>>())?;
    if saved.enabled {
        let id = saved.id.clone();
        spawn(async move { check_one(&id).await });
    }
    Ok(saved)
}

pub fn toggle_watch(id: &str) -> Result<(), String> {
    let list = read_watches()?;
    let Some(old) = list.iter().find(|w| w.id == id).cloned() else {
        return Ok(());
    };
    // What was assigned while it was paused is not asked about: it starts over from what is assigned on resume.
    write_watches(&list.into_iter().map(|w| if w.id == id { Watch { enabled: !w.enabled, primed: if w.enabled { w.primed } else { false }, ..w } } else { w }).collect::<Vec<_>>())?;
    if !old.enabled {
        let id = id.to_owned();
        spawn(async move { check_one(&id).await });
    }
    Ok(())
}

pub fn dismiss(id: &str, ticket: &str) -> Result<(), String> {
    update(id, |w| Watch { waiting: w.waiting.iter().filter(|t| t.ticket.id != ticket).cloned().collect(), ..w })
}

pub async fn check_watch(id: &str) -> Result<(), String> {
    forget_sources();
    find(id)?;
    check_one(id).await;
    Ok(())
}

/// Makes the worktree for a ticket and opens the agent in it, in a tab behind the one on screen. The ticket is one that is
/// waiting, or one the last check listed in a state that is asked about, and not one already started (`force` skips both).
/// It starts in `project_id`, one of the entry's `targets_of`, or in the entry's own project.
pub async fn start_ticket(id: &str, ticket_id: &str, force: bool, project_id: Option<&str>) -> Result<(String, String), String> {
    let key = format!("{id}\n{ticket_id}");
    if !STARTING.with(|starting| starting.borrow_mut().insert(key.clone())) {
        return Err("That one is already starting.".into());
    }
    let done = start_ticket_once(id, ticket_id, force, project_id).await;
    STARTING.with(|starting| starting.borrow_mut().remove(&key));
    done
}

/// The ticket to start and its kind, or why it isn't one that starts: the refusals that come before anything is made.
fn startable(w: &Watch, ticket_id: &str, force: bool) -> Result<(Ticket, Option<String>), String> {
    if w.started.iter().any(|(started, _)| started == ticket_id) && !force {
        return Err("That ticket was already started. Pass force=true to start it again.".into());
    }
    let waiting = w.waiting.iter().find(|t| t.ticket.id == ticket_id);
    let listed = w.tickets.as_ref().and_then(|tickets| tickets.iter().find(|t| t.ticket.id == ticket_id));
    let ticket: Ticket = waiting.map(|t| t.ticket.clone()).or_else(|| listed.map(|t| t.ticket.clone())).ok_or("That ticket isn't waiting or assigned to you any more.")?;
    if let (Some(listed), false) = (listed, force)
        && let Some(started) = started_elsewhere(listed)
    {
        return Err(format!("It was already started: {started}. Pass force=true to start it anyway."));
    }
    if let (None, Some(listed), false) = (waiting, listed, force)
        && !w.asks(listed)
    {
        let titles = match &w.prefixes {
            Some(prefixes) if !prefixes.is_empty() => format!(" or titles not starting with {}", prefixes.join(", ")),
            _ => String::new(),
        };
        return Err(format!("This entry doesn't ask about tickets in {}{titles}. Pass force=true to start it anyway.", listed.state));
    }
    Ok((ticket, listed.and_then(|t| t.kind.clone())))
}

/// The project a ticket starts in: `project_id`, one of the entry's `targets_of`, or the entry's own.
async fn start_project(w: &Watch, project_id: Option<&str>) -> Result<Project, String> {
    let projects = Project::list(&host::projects().await?);
    let want = project_id.unwrap_or(&w.project_id);
    if !targets_of(w, &projects).await.iter().any(|p| p.id == want) {
        return Err("That project isn't one this entry's tickets can start in.".to_owned());
    }
    projects.into_iter().find(|p| p.id == want).ok_or_else(|| "The project is no longer in Wings.".to_owned())
}

/// A start that passes the refusals that need no side effect: the entry, the ticket and the project it starts in.
pub struct StartPlan {
    pub w: Watch,
    pub ticket: Ticket,
    pub project: Project,
}

pub async fn plan_start(id: &str, ticket_id: &str, force: bool, project_id: Option<&str>) -> Result<StartPlan, String> {
    let w = find(id)?;
    let (ticket, _) = startable(&w, ticket_id, force)?;
    let project = start_project(&w, project_id).await?;
    Ok(StartPlan { w, ticket, project })
}

async fn start_ticket_once(id: &str, ticket_id: &str, force: bool, project_id: Option<&str>) -> Result<(String, String), String> {
    let w = find(id)?;
    let (ticket, kind) = startable(&w, ticket_id, force)?;
    let at = host::now() as i64;
    let mut made_worktree = false;
    let mut target = String::new();
    let mut made_in: Option<String> = None;
    let mut issue_problem: Option<String> = None;
    let started = async {
        let project = &start_project(&w, project_id).await?;
        if let Some(problem) = disk_problem(&Disk::list(&host::disks().await?), &project.path, w.min_free_gb) {
            return Err(problem);
        }
        let branch = branch_name(&ticket);
        // The base is a branch of the entry's own repo: another project starts from its default.
        let mut params = json!({ "projectId": project.id, "branch": branch });
        if let (Some(base), true) = (&w.base, project.id == w.project_id) {
            params["base"] = base.clone().into();
        }
        let made = host::call("createWorktree", params).await?;
        made_worktree = true;
        made_in = made["projectId"].as_str().map(str::to_owned);
        target = branch.clone();
        let (key, problem) = track_issue(&project.id, &AssignedTicket { ticket: ticket.clone(), state: String::new(), kind: kind.clone(), branch: None, issue: None }, &branch).await;
        issue_problem = problem;
        let filled = fill_prompt(&w.prompt, &ticket)?;
        let prompt = key.map_or(filled.clone(), |key| with_issue_key(&filled, &key));
        let opened = host::call("openPane", json!({ "command": w.agent.command(), "args": [prompt], "cwd": made["path"], "placement": "tab" })).await?;
        Ok(opened["paneId"].as_str().map(str::to_owned))
    }
    .await;
    let run = match started {
        Ok(pane_id) => Run { at, pane_id, ok: true, error: None },
        Err(error) => Run { at, pane_id: None, ok: false, error: Some(if made_worktree { format!("Made the worktree, but couldn't start the agent: {error}") } else { error }) },
    };
    // A ticket whose worktree exists isn't offered again: Start would only fail on the branch.
    let done = run.ok || made_worktree;
    update(id, |x| Watch {
        last_run: Some(run.clone()),
        waiting: if done { x.waiting.iter().filter(|t| t.ticket.id != ticket_id).cloned().collect() } else { x.waiting.clone() },
        started: if done { with_started(&x.started, ticket_id, at) } else { x.started.clone() },
        ..x
    })?;
    if !run.ok {
        return Err(run.error.unwrap_or_default());
    }
    let mut note = json!({
        "title": format!("Started {}", ticket.title),
        "body": format!("{} on {target}{}", w.agent.label(), issue_problem.map_or(String::new(), |problem| format!(". {problem}."))),
        "actions": [{ "label": "Open Schedules", "open": { "view": "schedules" } }],
    });
    if let Some(project) = made_in {
        note["projectId"] = project.into();
    }
    drop(host::call("notify", note));
    Ok((ticket.title, target))
}

/// The program, key and GitHub host to set up for an entry's CLI.
fn cli_of(id: &str) -> Result<(Program, Option<String>, Option<String>), String> {
    let w = find(id)?;
    let (source, command) = w.source.as_ref().and_then(|source| list_command(source).map(|command| (source.clone(), command))).ok_or("There is no CLI to set up for this entry.")?;
    let github = if let Source::GitHub { host, .. } = &source { Some(host.clone()) } else { None };
    Ok((Program::of(command.0).unwrap_or(Program::Az), key_of(&source), github))
}

pub async fn install(id: &str) -> Result<(), String> {
    cli_of(id)?.0.install().await
}

/// The one-time code of a gh login in a line it printed, like `code: ABCD-1234` or `code (ABCD-1234)`.
fn code_in(line: &str) -> Option<&str> {
    line.match_indices("code").find_map(|(at, found)| {
        let rest = &line[at + found.len()..];
        let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix(" ("))?.trim_start();
        let code = rest.get(..9)?;
        let ok = code.bytes().enumerate().all(|(i, b)| if i == 4 { b == b'-' } else { b.is_ascii_uppercase() || b.is_ascii_digit() });
        ok.then_some(code)
    })
}

/// The first address on `host` in a line, to open for the login.
fn url_in<'a>(line: &'a str, host: &str) -> Option<&'a str> {
    let start = line.find(&format!("https://{host}/"))?;
    let url = line[start..].split_whitespace().next()?;
    (url.len() > format!("https://{host}/").len()).then_some(url)
}

pub async fn sign_in(id: &str) -> Result<(), String> {
    let (program, key, github) = cli_of(id)?;
    let result = program
        .sign_in(key.as_deref(), move |line| {
            // gh doesn't open the browser without a terminal: it prints a one-time code and a URL.
            let Some(github) = &github else {
                return;
            };
            if let Some(code) = code_in(line) {
                SIGN_IN_CODE.with(|slot| *slot.borrow_mut() = Some(code.to_owned()));
                changed();
            }
            if let Some(url) = url_in(line, github) {
                host::open_url(url);
            }
        })
        .await;
    SIGN_IN_CODE.with(|slot| *slot.borrow_mut() = None);
    changed();
    result?;
    // After a sign-in the remotes are read again and the entries checked.
    forget_sources();
    check_all().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_and_address_of_a_github_login_are_read_from_what_gh_prints() {
        assert_eq!(code_in("! First copy your one-time code: 1A2B-3C4D"), Some("1A2B-3C4D"));
        assert_eq!(code_in("Your one-time code (ABCD-1234) is on the clipboard"), Some("ABCD-1234"));
        assert_eq!(code_in("no code here"), None);
        assert_eq!(code_in("code: abcd-1234"), None);
        assert_eq!(url_in("- Press Enter to open https://github.com/login/device in your browser", "github.com"), Some("https://github.com/login/device"));
        assert_eq!(url_in("see https://github.com/", "github.com"), None);
        assert_eq!(url_in("https://example.com/x", "github.com"), None);
    }
}
