//! What this chat's Claude scheduled with `CronCreate` (also `/loop`) and `ScheduleWakeup`: readable tool rows, and the
//! list the side panel shows with a way to stop one. Claude Code 2.1.330 run headless sends no `background_tasks_changed`
//! entry for them (probed: only the tool results say), so the list is what the results say: `CronCreate` answers
//! `{ id, humanSchedule, recurring, durable, nextFireAt }`, `CronList` `{ jobs: [{ id, cron, humanSchedule, prompt,
//! recurring, durable, nextFireAt }] }`, `CronDelete` `{ id }` and `ScheduleWakeup` `{ scheduledFor, … }` (all in ms
//! since the epoch). They sit on the message's `tool_use_result`, which a transcript names `toolUseResult`: a replay of
//! one adds nothing, since a session that starts again has none of the session-only jobs left.

use serde_json::Value;
use wings_plugin::format::truncate;

use crate::session::{Item, Session};

/// One scheduled task, or the pending wakeup of a loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: String,
    /// The prompt it runs, or why a wakeup was set.
    pub what: String,
    /// When, as Claude Code says it ("Every 10 minutes").
    pub when: String,
    pub recurring: bool,
    /// The next run (ms since the epoch), 0 when unknown.
    pub next: u64,
    pub wakeup: bool,
    /// Its stop was asked for.
    pub stopping: bool,
}

/// The wakeup's id here: Claude Code gives it none.
pub const WAKEUP: &str = "wakeup";

#[derive(Debug, Default, Clone)]
pub struct Scheduled {
    pub jobs: Vec<Job>,
    /// The input of each `CronCreate` and `ScheduleWakeup` call, by call id, until its result comes.
    calls: Vec<(String, Value)>,
}

impl Scheduled {
    /// A call is made: its input holds what its result won't.
    pub fn called(&mut self, tool: &str, id: &str, input: &Value) {
        if matches!(tool, "CronCreate" | "ScheduleWakeup") {
            self.calls.push((id.to_owned(), input.clone()));
        }
    }

    /// The words that ask for job `id` to be stopped, once. None when it isn't listed or is stopping already.
    pub fn stop(&mut self, id: &str) -> Option<String> {
        let job = self.jobs.iter_mut().find(|job| job.id == id && !job.stopping)?;
        job.stopping = true;
        Some(match job.wakeup {
            true => "Cancel the pending wakeup of the loop: call ScheduleWakeup with stop: true.".to_owned(),
            false => format!("Cancel the scheduled task {id} with CronDelete."),
        })
    }
}

/// What a tool call does, in one line, for the four tools; None for any other.
pub fn summary(tool: &str, input: &Value) -> Option<String> {
    let text = |key: &str| input[key].as_str().map(|said| said.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|said| !said.is_empty());
    Some(truncate(
        &match tool {
            "CronCreate" => line(&text("prompt")?, text("cron").as_deref().unwrap_or_default(), input["recurring"] != false),
            "CronList" => "List the scheduled tasks".to_owned(),
            "CronDelete" => format!("Cancel scheduled task {}", text("id")?),
            "ScheduleWakeup" if input["stop"] == true => "Stop the loop's pending wakeup".to_owned(),
            "ScheduleWakeup" => {
                let after = format!("Wake up in {}", span(input["delaySeconds"].as_u64()?));
                match text("reason") {
                    Some(reason) => format!("{after} · {reason}"),
                    None => after,
                }
            }
            _ => return None,
        },
        160,
    ))
}

/// "say hi · Every 10 minutes · recurring".
fn line(what: &str, when: &str, recurring: bool) -> String {
    let when = if when.is_empty() { String::new() } else { format!(" · {when}") };
    format!("{what}{when} · {}", if recurring { "recurring" } else { "once" })
}

/// "30 s", "5 min", "1 h 30 min".
fn span(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds.div_ceil(60)),
        _ => match seconds % 3600 / 60 {
            0 => format!("{} h", seconds / 3600),
            minutes => format!("{} h {minutes} min", seconds / 3600),
        },
    }
}

/// A tool result is heard: `line` is the user message that holds it.
pub fn answered(session: &mut Session, line: &Value) {
    let Some(block) = line["message"]["content"].as_array().and_then(|blocks| blocks.iter().find(|block| block["type"] == "tool_result")) else {
        return;
    };
    let id = block["tool_use_id"].as_str().unwrap_or_default();
    let Some(name) = session.items.iter().rev().find_map(|item| match item {
        Item::Tool(tool) if tool.id == id => Some(tool.name.clone()),
        _ => None,
    }) else {
        return;
    };
    let call = session.scheduled.calls.iter().position(|(call, _)| call == id).map(|at| session.scheduled.calls.remove(at).1).unwrap_or(Value::Null);
    let result = &line["tool_use_result"];
    // A refused or failed call changed nothing.
    if block["is_error"] == true || !result.is_object() {
        for job in &mut session.scheduled.jobs {
            job.stopping = false;
        }
        return;
    }
    let jobs = &mut session.scheduled.jobs;
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
    let job = |value: &Value, what: String| Job {
        id: text(value, "id"),
        what,
        when: text(value, "humanSchedule"),
        recurring: value["recurring"] != false,
        next: value["nextFireAt"].as_f64().unwrap_or(0.0) as u64,
        wakeup: false,
        stopping: false,
    };
    let mut said = None;
    match name.as_str() {
        "CronCreate" => {
            let made = job(result, text(&call, "prompt"));
            said = Some(self::line(&made.what.split_whitespace().collect::<Vec<_>>().join(" "), &made.when, made.recurring));
            jobs.retain(|old| old.id != made.id);
            jobs.push(made);
        }
        // The list is whole, and has the loop's pending wakeup in it as a one-shot task.
        "CronList" => *jobs = result["jobs"].as_array().into_iter().flatten().map(|listed| job(listed, text(listed, "prompt"))).collect(),
        "CronDelete" => jobs.retain(|old| old.id != text(result, "id")),
        "ScheduleWakeup" => {
            jobs.retain(|old| !old.wakeup);
            if call["stop"] != true && result["scheduledFor"].is_number() {
                let what = text(&call, "reason");
                let what = if what.is_empty() { text(&call, "prompt") } else { what };
                jobs.push(Job { id: WAKEUP.into(), what, when: "Loop wakeup".into(), recurring: false, next: result["scheduledFor"].as_f64().unwrap_or(0.0) as u64, wakeup: true, stopping: false });
            }
        }
        _ => {}
    }
    for job in &mut session.scheduled.jobs {
        job.stopping = false;
    }
    // The row of a created task says when, as Claude Code does, in place of the cron expression.
    if let Some(said) = said
        && let Some(tool) = session.items.iter_mut().rev().find_map(|item| match item {
            Item::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    {
        tool.summary = truncate(&said, 160);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(session: &mut Session, id: &str, name: &str, input: Value) {
        session.hear(&json!({ "type": "assistant", "message": { "model": "m", "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }));
    }

    fn result(session: &mut Session, id: &str, text: &str, structured: Value) {
        session.hear(&json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": text }] }, "tool_use_result": structured }));
    }

    fn rows(session: &Session) -> Vec<String> {
        session.items.iter().filter_map(|item| if let Item::Tool(tool) = item { Some(tool.summary.clone()) } else { None }).collect()
    }

    #[test]
    fn the_four_tools_have_readable_rows() {
        let mut session = Session::default();
        call(&mut session, "a", "CronCreate", json!({ "cron": "*/10 * * * *", "prompt": "say hi", "recurring": true }));
        call(&mut session, "b", "CronList", json!({}));
        call(&mut session, "c", "ScheduleWakeup", json!({ "delaySeconds": 300, "prompt": "check", "reason": "wait for CI" }));
        call(&mut session, "d", "CronDelete", json!({ "id": "5a8f492d" }));
        assert_eq!(rows(&session), ["say hi · */10 * * * * · recurring", "List the scheduled tasks", "Wake up in 5 min · wait for CI", "Cancel scheduled task 5a8f492d"]);
        let mut once = Session::default();
        call(&mut once, "a", "CronCreate", json!({ "cron": "30 14 * * *", "prompt": "deploy", "recurring": false }));
        assert_eq!(rows(&once), ["deploy · 30 14 * * * · once"]);
    }

    /// What Claude Code 2.1.330 printed for the calls of one probe, with the id as it gave it.
    #[test]
    fn the_results_keep_the_list_and_the_created_row_says_when() {
        let mut session = Session::default();
        call(&mut session, "a", "CronCreate", json!({ "cron": "*/10 * * * *", "prompt": "say hi", "recurring": true }));
        result(&mut session, "a", "Scheduled recurring job 5a8f492d", json!({ "id": "5a8f492d", "humanSchedule": "Every 10 minutes", "recurring": true, "durable": false, "nextFireAt": 1795422706124.66 }));
        assert_eq!(rows(&session), ["say hi · Every 10 minutes · recurring"]);
        let job = &session.scheduled.jobs[0];
        assert_eq!((job.id.as_str(), job.what.as_str(), job.when.as_str(), job.recurring, job.next), ("5a8f492d", "say hi", "Every 10 minutes", true, 1795422706124));
        call(&mut session, "w", "ScheduleWakeup", json!({ "delaySeconds": 300, "prompt": "check", "reason": "probe" }));
        result(&mut session, "w", "Next wakeup scheduled", json!({ "scheduledFor": 1795422540000u64, "clampedDelaySeconds": 300, "wasClamped": false }));
        assert_eq!(session.scheduled.jobs.iter().map(|job| (job.id.as_str(), job.wakeup)).collect::<Vec<_>>(), [("5a8f492d", false), (WAKEUP, true)]);
        // Stopping asks Claude for it, and the result that follows takes it off the list.
        assert_eq!(session.scheduled.stop("5a8f492d").as_deref(), Some("Cancel the scheduled task 5a8f492d with CronDelete."));
        assert_eq!(session.scheduled.stop("5a8f492d"), None);
        call(&mut session, "d", "CronDelete", json!({ "id": "5a8f492d" }));
        result(&mut session, "d", "Cancelled job 5a8f492d.", json!({ "id": "5a8f492d" }));
        assert_eq!(session.scheduled.jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(), [WAKEUP]);
        // A call that stops the loop clears its wakeup; the list is whole.
        call(&mut session, "s", "ScheduleWakeup", json!({ "stop": true }));
        result(&mut session, "s", "Stopped", json!({ "stopped": true }));
        assert!(session.scheduled.jobs.is_empty());
        call(&mut session, "l", "CronList", json!({}));
        result(&mut session, "l", "x", json!({ "jobs": [{ "id": "q1", "cron": "0 9 * * *", "humanSchedule": "Every day at 9:00", "prompt": "standup", "recurring": true, "durable": true, "nextFireAt": 1795440000000u64 }] }));
        assert_eq!(session.scheduled.jobs.iter().map(|job| (job.id.as_str(), job.what.as_str())).collect::<Vec<_>>(), [("q1", "standup")]);
    }
}
