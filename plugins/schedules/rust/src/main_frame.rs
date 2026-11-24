//! The main script: a timer that starts the agents that are due, the entries that watch for assigned tickets, the tools
//! Claude gets, and the changes the Schedules page asks for. The pages read what is saved themselves and ask main to
//! change it, so main is the only writer. A tick is a few comparisons, so the timers run while Wings is in the
//! background too: that is when a run, or a new ticket, is wanted.

use crate::{
    ASK, DONE, STATE, WANT,
    approvals,
    assigned::{Project, Watch, WatchDraft},
    schedule::{Draft, Run, RunIn, Schedule, When, claim, clean_draft, clean_prompt, describe_when, due_at, on_start, refuse_past},
    store::{read_schedules, read_watches, save_schedules, save_watches},
    tools,
    watches::{CHECK_MS, add_watch, check_all, check_one, check_watch, dismiss, install, remove_watch, save_watch, sign_in, sign_in_code, start_ticket, targets_of, toggle_watch},
};
use serde_json::{Map, Value, json};
use std::cell::Cell;
use wings_plugin::{host, messages, task::spawn};

const TICK_MS: u32 = 30_000;

fn base36(mut n: u64) -> String {
    let mut digits = Vec::new();
    while n > 0 || digits.is_empty() {
        digits.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

thread_local! {
    static SEED: Cell<u64> = const { Cell::new(0) };
}

/// The id of a new entry: the time, then six characters that differ between two made in one millisecond.
pub fn new_id(now: i64) -> String {
    let random = SEED.with(|seed| {
        let mut x = seed.get() ^ (now as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        seed.set(x);
        x
    });
    format!("{}{}", base36(now as u64), &base36(random % 36u64.pow(6) + 36u64.pow(5))[..6])
}

pub fn write_schedules(list: &[Schedule]) -> Result<(), String> {
    save_schedules(list)?;
    changed();
    Ok(())
}

pub fn write_watches(list: &[Watch]) -> Result<(), String> {
    save_watches(list)?;
    changed();
    Ok(())
}

/// Tells the pages, and redraws the project sections.
pub fn changed() {
    spawn(tell_state());
    spawn(sync_sections());
}

/// What the pages can't read themselves: the projects each entry's tickets can start in, a sign-in that is running, and
/// what agents are waiting for the user to allow.
async fn tell_state() {
    let projects = Project::list(&host::projects().await.unwrap_or_default());
    let mut targets = Map::new();
    for w in read_watches().unwrap_or_default() {
        targets.insert(w.id.clone(), targets_of(&w, &projects).await.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>().into());
    }
    messages::tell(STATE, &json!({ "targets": targets, "signInCode": sign_in_code(), "approvals": approvals::waiting() }));
}

/// Shows each project the schedules and "assigned" entries that run in it, and removes the section from the rest.
async fn sync_sections() {
    let (Ok(schedules), Ok(watches), Ok(projects)) = (read_schedules(), read_watches(), host::projects().await) else {
        return;
    };
    for project in Project::list(&projects) {
        let mut rows: Vec<Value> = Vec::new();
        for s in schedules.iter().filter(|s| s.project_id == project.id) {
            let mut row = json!({ "id": format!("s:{}", s.id), "label": s.title(), "detail": describe_when(&s.when), "open": { "view": "schedules" } });
            if !s.enabled {
                row["badge"] = json!({ "text": "Paused" });
            }
            rows.push(row);
        }
        for w in watches.iter().filter(|w| w.project_id == project.id) {
            let detail = format!("{} · {}", w.agent.label(), w.source.as_ref().map_or("Checking", |source| source.label()));
            let mut row = json!({ "id": format!("w:{}", w.id), "label": w.title(), "detail": detail, "open": { "view": "schedules", "args": { "waiting": w.id } } });
            if !w.waiting.is_empty() {
                row["badge"] = json!({ "text": format!("{} waiting", w.waiting.len()), "tone": "info" });
            } else if !w.enabled {
                row["badge"] = json!({ "text": "Paused" });
            }
            rows.push(row);
        }
        drop(host::set_project_section(&project.id, "schedules", &if rows.is_empty() { Value::Null } else { Value::Array(rows) }).await);
    }
}

/// Adds a schedule, or changes the one `id` names, and gives it as saved. Fails with what the form shows.
pub fn save_schedule(id: Option<&str>, draft: &Draft) -> Result<Schedule, String> {
    let clean = clean_draft(draft)?;
    let list = read_schedules()?;
    let now = host::now() as i64;
    let Some(id) = id else {
        refuse_past(&clean.when, now)?;
        let added = Schedule { id: new_id(now), name: clean.name, project_id: clean.project_id, agent: clean.agent, run_in: clean.run_in, prompt: clean.prompt, when: clean.when, enabled: true, anchor: now, last_run: None };
        write_schedules(&[list, vec![added.clone()]].concat())?;
        return Ok(added);
    };
    let old = list.iter().find(|s| s.id == id).ok_or("That schedule is gone.")?;
    if old.when != clean.when {
        refuse_past(&clean.when, now)?;
    }
    // A changed time counts from now, so it never runs for a time it was already past when you saved it.
    let saved = Schedule { name: clean.name, project_id: clean.project_id, agent: clean.agent, run_in: clean.run_in, prompt: clean.prompt, when: clean.when, anchor: now, ..old.clone() };
    write_schedules(&list.iter().map(|s| if s.id == id { saved.clone() } else { s.clone() }).collect::<Vec<_>>())?;
    Ok(saved)
}

pub fn remove_schedule(id: &str) -> Result<(), String> {
    write_schedules(&read_schedules()?.into_iter().filter(|s| s.id != id).collect::<Vec<_>>())
}

/// Starts the agent for `s`, in a new tab in its project. Never fails: a failure is the run's result. A run set for
/// the Claude chat that can't open one falls back to a terminal, and says why in the run's `error`, which stays on a run
/// that started.
async fn open_agent(s: &Schedule) -> Run {
    let at = host::now() as i64;
    let opened = async {
        let projects = Project::list(&host::projects().await?);
        let project = projects.iter().find(|p| p.id == s.project_id).ok_or("The project is no longer in Wings.")?;
        let mut fell_back = None;
        if s.run_in.for_agent(s.agent) == RunIn::Chat {
            match host::open_plugin_tab_in("claude-chat", "chat", &project.id, &json!({ "cwd": project.path, "prompt": s.prompt })).await {
                Ok(_) => return Ok::<(Option<String>, Option<String>), String>((None, None)),
                Err(error) => fell_back = Some(format!("Claude chat didn't open ({error}), so it ran in a terminal.")),
            }
        }
        // A terminal types the prompt into a shell: one line, and up to 4 KB, as a chat's longer one with line breaks is not.
        let prompt = clean_prompt(&s.prompt, RunIn::Terminal).map_err(|why| fell_back.as_ref().map_or(why.clone(), |first| format!("{first} The prompt can't run in a terminal: {why}")))?;
        let answer = host::OpenPane::new().command(s.agent.command(), &[prompt.as_str()]).cwd(&project.path).background().open()
            .await
            .map_err(|error| fell_back.as_ref().map_or(error.clone(), |why| format!("{why} The terminal failed too: {error}")))?;
        Ok((answer["paneId"].as_str().map(str::to_owned), fell_back))
    }
    .await;
    match opened {
        Ok((pane_id, error)) => Run { at, pane_id, ok: true, error },
        Err(error) => Run { at, pane_id: None, ok: false, error: Some(error) },
    }
}

fn record(id: &str, run: Run) {
    let Ok(list) = read_schedules() else {
        return;
    };
    if list.iter().any(|s| s.id == id) {
        drop(write_schedules(&list.into_iter().map(|s| if s.id == id { Schedule { last_run: Some(run.clone()), ..s } } else { s }).collect::<Vec<_>>()));
    }
}

/// Claims what is due, so it can't be due again, then starts each and tells you.
async fn tick() {
    let now = host::now() as i64;
    let Ok(list) = read_schedules() else {
        return;
    };
    let mut found: Vec<Schedule> = Vec::new();
    let next: Vec<Schedule> = list
        .into_iter()
        .map(|s| {
            let Some(at) = due_at(&s, now) else {
                return s;
            };
            found.push(s.clone());
            claim(&s, at, now)
        })
        .collect();
    if !found.is_empty() && write_schedules(&next).is_err() {
        return;
    }
    for s in found {
        let run = open_agent(&s).await;
        record(&s.id, run.clone());
        let projects = Project::list(&host::projects().await.unwrap_or_default());
        let project = projects.iter().find(|p| p.id == s.project_id);
        let mut note = json!({
            "title": if run.ok { format!("Started {}", s.title()) } else { format!("Couldn't start {}", s.title()) },
            "body": if run.ok { format!("{} in {}", s.agent.label(), project.map_or("its project", |p| p.name.as_str())) } else { run.error.clone().unwrap_or_default() },
            "actions": [{ "label": "Open Schedules", "open": { "view": "schedules" } }],
        });
        if let Some(project) = project {
            note["projectId"] = project.id.clone().into();
        }
        drop(host::call("notify", note));
    }
}

fn toggle(id: &str) -> Result<(), String> {
    let now = host::now() as i64;
    write_schedules(&read_schedules()?.into_iter().map(|s| if s.id == id { Schedule { enabled: !s.enabled, anchor: if matches!(s.when, When::Once { .. }) { s.anchor } else { now }, ..s } } else { s }).collect::<Vec<_>>())
}

async fn run_now(id: &str) -> Result<(), String> {
    let s = read_schedules()?.into_iter().find(|x| x.id == id).ok_or("That schedule is gone.")?;
    let run = open_agent(&s).await;
    record(id, run.clone());
    if run.ok { Ok(()) } else { Err(run.error.unwrap_or_default()) }
}

/// A page's ask, done, as its `ask` has it: the op and its arguments.
async fn run_op(ask: &Value) -> Result<(), String> {
    let text = |name: &str| ask[name].as_str().unwrap_or_default().to_owned();
    let id = text("entry");
    match ask["op"].as_str().unwrap_or_default() {
        "save" => save_schedule(ask["entry"].as_str(), &Draft::from_json(&ask["draft"]).ok_or("The form is incomplete.")?).map(drop),
        "toggle" => toggle(&id),
        "remove" => remove_schedule(&id),
        "runNow" => run_now(&id).await,
        "saveWatch" => {
            let draft = WatchDraft::from_json(&ask["draft"]).ok_or("The form is incomplete.")?;
            if ask["worktree"].is_object() {
                let mut params = ask["worktree"]["change"].clone();
                params["projectId"] = ask["worktree"]["projectId"].clone();
                host::call("setWorktreeSetup", params).await?;
            }
            match ask["entry"].as_str() {
                None => {
                    let saved = add_watch(&draft, false)?;
                    spawn(async move { check_one(&saved.id).await });
                    Ok(())
                }
                Some(id) => save_watch(id, &draft).map(drop),
            }
        }
        "toggleWatch" => toggle_watch(&id),
        "removeWatch" => remove_watch(&id),
        "checkWatch" => check_watch(&id).await,
        "startTicket" => start_ticket(&id, &text("ticket"), ask["force"] == true, ask["projectId"].as_str()).await.map(drop),
        "dismissWaiting" => dismiss(&id, &text("ticket")),
        "install" => install(&id).await,
        "signIn" => sign_in(&id).await,
        "answerApproval" => approvals::answer(&text("approval"), ask["allow"] == true),
        other => Err(format!("The plugin has no {other}.")),
    }
}

async fn serve(ask: Value) {
    let error = run_op(&ask).await.err();
    messages::tell(DONE, &json!({ "id": ask["id"], "error": error }));
}

#[unsafe(no_mangle)]
pub fn start() {
    messages::on(ASK, |ask| spawn(serve(ask.clone())));
    messages::on(WANT, |_| changed());
    host::on_projects(|_| changed());
    tools::register_all();
    // A schedule that came due while Wings was closed counts from now, then the timer starts.
    let now = host::now() as i64;
    if let Ok(list) = read_schedules() {
        let next: Vec<Schedule> = list.iter().map(|s| on_start(s, now)).collect();
        if next != list {
            drop(save_schedules(&next));
        }
    }
    spawn(async {
        loop {
            tick().await;
            host::sleep(TICK_MS).await;
        }
    });
    spawn(async {
        loop {
            check_all().await;
            host::sleep(CHECK_MS).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::task::block_on;

    fn schedule(agent: crate::schedule::Agent, run_in: RunIn) -> Schedule {
        Schedule {
            id: "a".into(),
            name: String::new(),
            project_id: "p".into(),
            agent,
            run_in,
            prompt: "Review the open PRs".into(),
            when: When::Every { hours: 1 },
            enabled: true,
            anchor: 0,
            last_run: None,
        }
    }

    /// Wings with the project `p` in `/work`, answering `openView` with `chat` and `openPane` with `pane`, keeping every call.
    fn wings(chat: Result<Value, String>, pane: Result<Value, String>) -> Rc<RefCell<Vec<(String, Value)>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            match method {
                "projects" => Ok(json!([{ "id": "p", "path": "/work", "name": "Work" }])),
                "openView" => chat.clone(),
                "openPane" => pane.clone(),
                _ => Ok(Value::Null),
            }
        });
        calls
    }

    fn methods(calls: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<String> {
        calls.borrow().iter().map(|(method, _)| method.clone()).filter(|method| method != "projects").collect()
    }

    #[test]
    fn a_claude_chat_run_opens_the_chat_tab_in_the_project_with_the_prompt_and_starts_no_terminal() {
        let calls = wings(Ok(Value::Null), Ok(json!({ "paneId": "9" })));
        let run = block_on(open_agent(Box::leak(Box::new(schedule(crate::schedule::Agent::Claude, RunIn::Chat)))));
        assert_eq!((run.ok, run.error, run.pane_id), (true, None, None));
        assert_eq!(methods(&calls), ["openView"]);
        let (_, params) = calls.borrow().iter().find(|(method, _)| method == "openView").cloned().unwrap();
        assert_eq!(params, json!({ "plugin": "claude-chat", "id": "chat", "as": "tab", "args": { "cwd": "/work", "prompt": "Review the open PRs" }, "newTab": true, "projectId": "p" }));
    }

    #[test]
    fn a_terminal_run_and_any_other_agent_open_a_pane_as_before() {
        for (agent, run_in, command) in [
            (crate::schedule::Agent::Claude, RunIn::Terminal, "claude"),
            (crate::schedule::Agent::OpenCode, RunIn::Chat, "opencode --prompt"),
        ] {
            let calls = wings(Ok(Value::Null), Ok(json!({ "paneId": "9" })));
            let run = block_on(open_agent(Box::leak(Box::new(schedule(agent, run_in)))));
            assert_eq!((run.ok, run.error, run.pane_id), (true, None, Some("9".into())));
            assert_eq!(methods(&calls), ["openPane"]);
            let (_, params) = calls.borrow().iter().find(|(method, _)| method == "openPane").cloned().unwrap();
            assert_eq!(params, json!({ "command": command, "args": ["Review the open PRs"], "cwd": "/work", "placement": "tab", "background": true }));
        }
    }

    #[test]
    fn a_chat_that_cannot_open_falls_back_to_a_terminal_and_the_run_says_why() {
        let calls = wings(Err("claude-chat is turned off or not installed".into()), Ok(json!({ "paneId": "9" })));
        let run = block_on(open_agent(Box::leak(Box::new(schedule(crate::schedule::Agent::Claude, RunIn::Chat)))));
        assert_eq!(methods(&calls), ["openView", "openPane"]);
        assert!(run.ok && run.pane_id.as_deref() == Some("9"));
        let why = run.error.unwrap();
        assert!(why.contains("claude-chat is turned off or not installed") && why.contains("ran in a terminal"), "{why}");
        // Both failing is a failed run that names both.
        wings(Err("no chat".into()), Err("no terminal".into()));
        let run = block_on(open_agent(Box::leak(Box::new(schedule(crate::schedule::Agent::Claude, RunIn::Chat)))));
        let why = run.error.unwrap();
        assert!(!run.ok && why.contains("no chat") && why.contains("no terminal"), "{why}");
    }
}
