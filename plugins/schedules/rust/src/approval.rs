//! What the tools need to add or change a timed schedule: its timing from the tool's fields, and what the user is asked to
//! allow. A timed schedule starts an agent by itself, so an agent can't add one, or change what or when it runs, unless the
//! user allows it.

use crate::schedule::{Draft, EVERY_HOURS, When};
use serde_json::{Value, json};

/// The tools' day names, at their weekday number.
pub const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

#[derive(Default)]
pub struct WhenInput {
    pub every_hours: Option<i64>,
    pub time: Option<String>,
    pub days: Option<Vec<String>>,
}

impl WhenInput {
    pub fn of(input: &Value) -> Self {
        WhenInput {
            every_hours: input["every_hours"].as_i64(),
            time: input["time"].as_str().map(str::to_owned),
            days: input["days"].as_array().map(|days| days.iter().filter_map(|day| day.as_str().map(str::to_owned)).collect()),
        }
    }
}

/// The timing the tool's fields give, over `current` for a change. `time` without `days` keeps the current days, or is
/// every day for a new one. None when no field is given. Fails with what to fix.
pub fn when_from(input: &WhenInput, current: Option<&When>) -> Result<Option<When>, String> {
    if input.every_hours.is_some() && (input.time.is_some() || input.days.is_some()) {
        return Err("Give every_hours, or time with days, not both.".into());
    }
    if let Some(hours) = input.every_hours {
        if !EVERY_HOURS.contains(&hours) {
            return Err(format!("every_hours is one of {}.", EVERY_HOURS.map(|h| h.to_string()).join(", ")));
        }
        return Ok(Some(When::Every { hours }));
    }
    let daily = match current {
        Some(When::Daily { time, days }) => Some((time, days)),
        _ => None,
    };
    if input.time.is_none() && input.days.is_none() {
        return Ok(None);
    }
    if input.time.is_none() && daily.is_none() {
        return Err("days goes with a time of day: give time too.".into());
    }
    let time = input.time.clone().or_else(|| daily.map(|(time, _)| time.clone())).unwrap_or_default();
    let days = match &input.days {
        Some(names) => names.iter().map(|name| DAY_NAMES.iter().position(|d| d == name).map_or(-1, |at| at as i64)).collect(),
        None => daily.map_or_else(|| (0..7).collect(), |(_, days)| days.clone()),
    };
    Ok(Some(When::Daily { time, days }))
}

/// What the user is asked to allow.
#[derive(Clone)]
pub struct Approval {
    pub id: String,
    /// The agent session that asks, or its pane's project, or none when neither is known.
    pub asker: Option<String>,
    pub project_name: String,
    /// The schedule as it is, for a change; none for a new one.
    pub current: Option<Draft>,
    pub proposed: Draft,
}

impl Approval {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Approval {
            id: value["id"].as_str()?.to_owned(),
            asker: value["asker"].as_str().map(str::to_owned),
            project_name: value["projectName"].as_str()?.to_owned(),
            current: Draft::from_json(&value["current"]),
            proposed: Draft::from_json(&value["proposed"])?,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "asker": self.asker, "projectName": self.project_name, "current": self.current.as_ref().map(Draft::to_json), "proposed": self.proposed.to_json() })
    }
}

/// Whether saving `next` over `prev` changes what runs, where or when. A new name alone doesn't need the user.
pub fn needs_approval(prev: &Draft, next: &Draft) -> bool {
    prev.project_id != next.project_id || prev.agent != next.agent || prev.prompt != next.prompt || prev.when != next.when
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Agent;

    fn daily() -> When {
        When::Daily { time: "09:00".into(), days: vec![1, 2, 3, 4, 5] }
    }

    fn time(t: &str) -> WhenInput {
        WhenInput { time: Some(t.into()), ..Default::default() }
    }

    #[test]
    fn a_time_with_no_days_runs_every_day_or_keeps_a_daily_schedules_days() {
        assert_eq!(when_from(&time("09:00"), None).unwrap(), Some(When::Daily { time: "09:00".into(), days: vec![0, 1, 2, 3, 4, 5, 6] }));
        assert_eq!(when_from(&time("18:30"), Some(&daily())).unwrap(), Some(When::Daily { time: "18:30".into(), days: vec![1, 2, 3, 4, 5] }));
        let weekend = WhenInput { days: Some(vec!["sat".into(), "sun".into()]), ..Default::default() };
        assert_eq!(when_from(&weekend, Some(&daily())).unwrap(), Some(When::Daily { time: "09:00".into(), days: vec![6, 0] }));
    }

    #[test]
    fn every_hours_takes_only_the_hours_the_page_offers_and_not_with_a_time() {
        let hours = |n| WhenInput { every_hours: Some(n), ..Default::default() };
        assert_eq!(when_from(&hours(4), Some(&daily())).unwrap(), Some(When::Every { hours: 4 }));
        assert!(when_from(&hours(5), None).unwrap_err().contains("one of 1, 2, 3, 4, 6, 8, 12, 24"));
        assert!(when_from(&WhenInput { every_hours: Some(4), time: Some("09:00".into()), days: None }, None).unwrap_err().contains("not both"));
        let monday = WhenInput { days: Some(vec!["mon".into()]), ..Default::default() };
        assert!(when_from(&monday, Some(&When::Every { hours: 4 })).unwrap_err().contains("give time too"));
        assert_eq!(when_from(&WhenInput::default(), Some(&daily())).unwrap(), None);
    }

    #[test]
    fn only_what_runs_where_or_when_needs_the_users_approval_not_a_new_name() {
        let draft = Draft { name: String::new(), project_id: "p1".into(), agent: Agent::Claude, prompt: "Check the board.".into(), when: daily() };
        let with = |change: &dyn Fn(&mut Draft)| {
            let mut next = draft.clone();
            change(&mut next);
            needs_approval(&draft, &next)
        };
        assert!(!with(&|d| d.name = "Board".into()));
        assert!(with(&|d| d.prompt = "Delete the board.".into()));
        assert!(with(&|d| d.agent = Agent::OpenCode));
        assert!(with(&|d| d.when = When::Daily { time: "03:00".into(), days: vec![1, 2, 3, 4, 5] }));
    }
}
