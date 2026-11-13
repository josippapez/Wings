//! The tools Claude, and `wings schedules <tool> key=value`, call. Each takes the input schema the manifest's `mcpTools`
//! has for it.

use crate::{
    approval::{StartAsk, WhenInput, needs_approval, when_from},
    approvals::{Answer, ask_approval, ask_start},
    assigned::{DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, Project, WatchDraft, describe_check, fill_prompt, resolve_project},
    list::format_list,
    main_frame::{remove_schedule, save_schedule},
    schedule::{Agent, Draft, Schedule, clean_draft, describe_next, describe_when, next_run, time_ok},
    store::{read_schedules, read_watches},
    watches::{add_watch, check_one, plan_start, remove_watch, save_watch, start_ticket},
};
use serde_json::Value;
use std::future::Future;
use wings_plugin::{host, tools};

const MANIFEST: &str = include_str!("../../wings-plugin.json");
const TIME_PATTERN: &str = "^([01]\\d|2[0-3]):[0-5]\\d$";

/// The tool's input schema from the manifest, without its `pattern`, which this SDK can't check: `time` is checked by hand.
fn schema(name: &str) -> Value {
    fn bare(schema: &mut Value) {
        match schema {
            Value::Object(map) => {
                map.remove("pattern");
                map.values_mut().for_each(bare);
            }
            Value::Array(list) => list.iter_mut().for_each(bare),
            _ => {}
        }
    }
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap_or_default();
    let mut found = manifest["contributes"]["mcpTools"].as_array().and_then(|list| list.iter().find(|tool| tool["name"] == name)).map_or(Value::Null, |tool| tool["inputSchema"].clone());
    bare(&mut found);
    found
}

fn register<F, Fut>(name: &str, run: F)
where
    F: Fn(Value, Value) -> Fut + 'static,
    Fut: Future<Output = Result<Value, String>> + 'static,
{
    tools::register(name, schema(name), run);
}

fn text(message: String) -> Result<Value, String> {
    Ok(Value::String(message))
}

/// What the schema's `pattern` says of `time`, with the SDK's words for it.
fn check_time(input: &Value) -> Result<(), String> {
    match input["time"].as_str() {
        Some(time) if !time_ok(time) => Err(format!("time must match the pattern {TIME_PATTERN}, this one is {}.", Value::String(time.to_owned()))),
        _ => Ok(()),
    }
}

/// Fails when the user didn't allow it. `none` says what didn't happen.
fn allowed(answer: Answer, none: &str) -> Result<(), String> {
    match answer {
        Answer::Allow => Ok(()),
        Answer::Deny => Err(format!("The user denied it in Wings. {none}")),
        Answer::Timeout => Err(format!("The user didn't answer in Wings within 100 seconds. {none} Ask them before you try again.")),
    }
}

fn describe_schedule(s: &Schedule) -> String {
    let at = if s.enabled { next_run(s) } else { None };
    format!("{}{}{}", describe_when(&s.when), at.map_or(String::new(), |at| format!(", next run {}", describe_next(at, host::now() as i64))), if s.enabled { "" } else { ", paused" })
}

async fn projects() -> Result<Vec<Project>, String> {
    Ok(Project::list(&host::projects().await?))
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value.as_array().map(|list| list.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
}

fn asks(states: Option<&Vec<String>>) -> String {
    match states {
        None => String::new(),
        Some(states) if states.is_empty() => " It asks about every state.".into(),
        Some(states) => format!(" It asks about: {}.", states.join(", ")),
    }
}

pub fn register_all() {
    register("list", |_, _| async {
        let (schedules, watches, projects) = (read_schedules()?, read_watches()?, projects().await?);
        text(format_list(&schedules, &watches, |id| projects.iter().find(|p| p.id == id).map_or_else(|| "missing project".into(), |p| p.name.clone()), host::now() as i64))
    });

    register("add", |input, context| async move {
        check_time(&input)?;
        let projects = projects().await?;
        let here = context["project"].as_object().map(|p| p.get("parentId").and_then(Value::as_str).or_else(|| p.get("id").and_then(Value::as_str)).unwrap_or_default().to_owned());
        let wanted = input["project"].as_str().map(str::to_owned).or(here).ok_or("Give project: this call didn't come from a pane in Wings.")?;
        let found = resolve_project(&projects, &wanted)?;
        let when = when_from(&WhenInput::of(&input), None)?.ok_or("Give every_hours, or time with days.")?;
        let agent = input["agent"].as_str().and_then(Agent::from_id).unwrap_or(Agent::Claude);
        let draft = clean_draft(&Draft { name: input["name"].as_str().unwrap_or_default().to_owned(), project_id: found.id.clone(), agent, prompt: input["prompt"].as_str().unwrap_or_default().to_owned(), when })?;
        allowed(ask_approval(context["session"]["name"].as_str().map(str::to_owned), &found.name, None, draft.clone(), &found.id).await, "Nothing changed.")?;
        let saved = save_schedule(None, &draft)?;
        text(format!("Added {} in {}: {}.", saved.id, found.name, describe_schedule(&saved)))
    });

    register("add_assigned", |input, _| async move {
        let projects = projects().await?;
        let found = resolve_project(&projects, input["project"].as_str().unwrap_or_default())?;
        let draft = WatchDraft {
            name: input["name"].as_str().map(str::to_owned),
            project_id: found.id.clone(),
            agent: input["agent"].as_str().and_then(Agent::from_id).unwrap_or(Agent::Claude),
            prompt: input["prompt"].as_str().unwrap_or(DEFAULT_PROMPT).to_owned(),
            min_free_gb: input["min_free_gb"].as_f64().unwrap_or(DEFAULT_MIN_FREE_GB as f64),
            states: strings(&input["states"]),
            prefixes: strings(&input["title_prefixes"]).unwrap_or_default(),
            base: input["base"].as_str().map(str::to_owned),
            // Only the user turns this on, on the Schedules page.
            agents_start: false,
        };
        let saved = add_watch(&draft, true)?;
        check_one(&saved.id).await;
        let w = read_watches()?.into_iter().find(|x| x.id == saved.id);
        text(format!("Added {} for {}. {}.{}", saved.id, found.name, describe_check(w.as_ref().and_then(|w| w.last_check.as_ref()), w.as_ref().and_then(|w| w.source.as_ref())), asks(w.as_ref().and_then(|w| w.states.as_ref()))))
    });

    register("check", |input, _| async move {
        let all = read_watches()?;
        let id = input["id"].as_str().filter(|id| !id.is_empty());
        let picked: Vec<_> = all.iter().filter(|w| id.map_or(w.enabled, |id| w.id == id)).collect();
        if let Some(id) = id {
            let Some(first) = picked.first() else {
                return Err(format!("No assigned entry {id}. Use list to see the ids."));
            };
            if !first.enabled {
                return Err("That entry is paused. Resume it on the Schedules page first.".into());
            }
        }
        if picked.is_empty() {
            return text("No assigned entries are enabled.".into());
        }
        let projects = projects().await?;
        let mut lines: Vec<String> = Vec::new();
        for w in picked {
            check_one(&w.id).await;
            let now = read_watches()?.into_iter().find(|x| x.id == w.id);
            let waiting = now.as_ref().map_or(0, |now| now.waiting.len());
            lines.push(format!(
                "{} · {} · {}{}",
                w.id,
                projects.iter().find(|p| p.id == w.project_id).map_or("missing project", |p| p.name.as_str()),
                describe_check(now.as_ref().and_then(|w| w.last_check.as_ref()), now.as_ref().and_then(|w| w.source.as_ref())),
                if waiting > 0 { format!(" · {waiting} waiting") } else { String::new() },
            ));
        }
        text(lines.join("\n"))
    });

    register("start", |input, context| async move {
        let project_id = match input["project"].as_str() {
            Some(wanted) => Some(resolve_project(&projects().await?, wanted)?.id.clone()),
            None => None,
        };
        let ticket = input["ticket"].as_str().unwrap_or_default();
        let (id, ticket, force) = (input["id"].as_str().unwrap_or_default(), ticket.strip_prefix('#').unwrap_or(ticket), input["force"] == true);
        let plan = plan_start(id, ticket, force, project_id.as_deref()).await?;
        // The plan refused what would fail first, so the user isn't asked about a start that couldn't happen.
        if !plan.w.agents_start {
            let entry_project = projects().await?.into_iter().find(|p| p.id == plan.w.project_id).map_or_else(|| "missing project".into(), |p| p.name);
            let start = StartAsk {
                ticket_id: plan.ticket.id.clone(),
                title: plan.ticket.title.clone(),
                starts_in: (plan.project.id != plan.w.project_id).then(|| plan.project.name.clone()),
                agent: plan.w.agent,
                prompt: fill_prompt(&plan.w.prompt, &plan.ticket)?,
            };
            allowed(ask_start(context["session"]["name"].as_str().map(str::to_owned), &entry_project, start, &plan.w.project_id).await, "Nothing started.")?;
        }
        let (title, target) = start_ticket(id, ticket, force, project_id.as_deref()).await?;
        text(format!("Started {title} on {target}."))
    });

    register("update", |input, context| async move {
        check_time(&input)?;
        let id = input["id"].as_str().unwrap_or_default().to_owned();
        let has = |name: &str| !input[name].is_null();
        if let Some(schedule) = read_schedules()?.into_iter().find(|x| x.id == id) {
            if has("min_free_gb") || has("states") || has("title_prefixes") || has("base") {
                return Err("min_free_gb, states, title_prefixes and base are for assigned entries. A schedule takes agent, prompt, name, every_hours, time and days.".into());
            }
            let current = schedule.draft();
            let proposed = clean_draft(&Draft {
                name: input["name"].as_str().map_or(current.name.clone(), str::to_owned),
                agent: input["agent"].as_str().and_then(Agent::from_id).unwrap_or(current.agent),
                prompt: input["prompt"].as_str().map_or(current.prompt.clone(), str::to_owned),
                when: when_from(&WhenInput::of(&input), Some(&current.when))?.unwrap_or(current.when.clone()),
                project_id: current.project_id.clone(),
            })?;
            if needs_approval(&current, &proposed) {
                let project_name = projects().await?.into_iter().find(|p| p.id == schedule.project_id).map_or_else(|| "missing project".into(), |p| p.name);
                allowed(ask_approval(context["session"]["name"].as_str().map(str::to_owned), &project_name, Some(current), proposed.clone(), &schedule.project_id).await, "Nothing changed.")?;
            }
            let saved = save_schedule(Some(&id), &proposed)?;
            return text(format!("Updated {id}: {}.", describe_schedule(&saved)));
        }
        let w = read_watches()?.into_iter().find(|x| x.id == id).ok_or_else(|| format!("No schedule or assigned entry {id}. Use list to see the ids."))?;
        if has("every_hours") || has("time") || has("days") {
            return Err("every_hours, time and days are for schedules. An assigned entry checks for tickets by itself.".into());
        }
        let saved = save_watch(
            &id,
            &WatchDraft {
                name: input["name"].as_str().map(str::to_owned).or(w.name.clone()),
                project_id: w.project_id.clone(),
                agent: input["agent"].as_str().and_then(Agent::from_id).unwrap_or(w.agent),
                prompt: input["prompt"].as_str().map_or(w.prompt.clone(), str::to_owned),
                min_free_gb: input["min_free_gb"].as_f64().unwrap_or(w.min_free_gb as f64),
                states: strings(&input["states"]).or(w.states.clone()),
                prefixes: strings(&input["title_prefixes"]).or(w.prefixes.clone()).unwrap_or_default(),
                base: input["base"].as_str().map(str::to_owned).or(w.base.clone()),
                agents_start: w.agents_start,
            },
        )?;
        let states = match &saved.states {
            Some(states) if !states.is_empty() => format!("asks about {}", states.join(", ")),
            _ => "every state".into(),
        };
        let titles = match &saved.prefixes {
            Some(prefixes) if !prefixes.is_empty() => format!(", titles starting with {}", prefixes.join(", ")),
            _ => String::new(),
        };
        text(format!("Updated {id}: {}, {states}{titles}, branches from {}.", saved.agent.id(), saved.base.as_deref().unwrap_or("the default branch")))
    });

    register("remove", |input, _| async move {
        let id = input["id"].as_str().unwrap_or_default();
        if read_schedules()?.iter().any(|x| x.id == id) {
            remove_schedule(id)?;
        } else if read_watches()?.iter().any(|x| x.id == id) {
            remove_watch(id)?;
        } else {
            return Err(format!("No entry {id}. Use list to see the ids."));
        }
        text(format!("Removed {id}."))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_tool_the_manifest_names_has_its_schema_without_a_pattern_and_time_is_checked_by_hand() {
        for name in ["list", "add", "add_assigned", "check", "start", "update", "remove"] {
            assert_eq!(schema(name)["type"], "object", "{name}");
        }
        assert!(schema("add")["properties"]["time"].get("pattern").is_none());
        assert_eq!(schema("add")["required"], json!(["prompt"]));
        assert_eq!(check_time(&json!({ "time": "09:00" })), Ok(()));
        assert_eq!(check_time(&json!({ "time": "9:00" })), Err("time must match the pattern ^([01]\\d|2[0-3]):[0-5]\\d$, this one is \"9:00\".".into()));
    }
}
