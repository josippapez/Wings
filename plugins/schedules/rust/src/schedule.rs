//! What a schedule is and when it runs next. All of it is plain date math in local time, so a tick costs nothing and a
//! daily time follows the clock on the wall, through a change to or from summer time.

use serde_json::{Value, json};
use wings_plugin::local_time::{at, local, start_of_day};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agent {
    Claude,
    OpenCode,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::OpenCode];

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.id() == id)
    }

    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::OpenCode => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::OpenCode => "OpenCode",
        }
    }

    /// What `openPane` starts, with the prompt as its one argument, and what the manifest's `panes` allows.
    pub fn command(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            // `opencode --help`: --prompt is the first message.
            Agent::OpenCode => "opencode --prompt",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum When {
    Every { hours: i64 },
    /// `time` is "HH:MM" and `days` are weekday numbers, 0 for Sunday.
    Daily { time: String, days: Vec<i64> },
}

impl When {
    pub fn from_json(value: &Value) -> Option<Self> {
        match value["kind"].as_str()? {
            "every" => Some(When::Every { hours: int(&value["hours"])? }),
            "daily" => Some(When::Daily { time: value["time"].as_str()?.to_owned(), days: value["days"].as_array()?.iter().filter_map(int).collect() }),
            _ => None,
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            When::Every { hours } => json!({ "kind": "every", "hours": hours }),
            When::Daily { time, days } => json!({ "kind": "daily", "time": time, "days": days }),
        }
    }
}

/// A number JS stored, which is a whole one here.
pub fn int(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_f64().map(|n| n as i64))
}

pub fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

#[derive(Clone, PartialEq, Debug)]
pub struct Run {
    pub at: i64,
    /// The pane the agent started in, when it did.
    pub pane_id: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
}

impl Run {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Run { at: int(&value["at"])?, pane_id: value["paneId"].as_str().map(str::to_owned), ok: value["ok"] == true, error: value["error"].as_str().map(str::to_owned) })
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({ "at": self.at, "ok": self.ok });
        if let Some(pane) = &self.pane_id {
            out["paneId"] = pane.clone().into();
        }
        if let Some(error) = &self.error {
            out["error"] = error.clone().into();
        }
        out
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Schedule {
    pub id: String,
    /// Left empty, the list shows the start of the prompt.
    pub name: String,
    pub project_id: String,
    pub agent: Agent,
    pub prompt: String,
    pub when: When,
    pub enabled: bool,
    /// Where the next run is counted from: when it was saved, resumed or last started, or when Wings started and found it overdue.
    pub anchor: i64,
    pub last_run: Option<Run>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Draft {
    pub name: String,
    pub project_id: String,
    pub agent: Agent,
    pub prompt: String,
    pub when: When,
}

impl Schedule {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Schedule {
            id: value["id"].as_str()?.to_owned(),
            name: text(&value["name"]),
            project_id: text(&value["projectId"]),
            agent: Agent::from_id(value["agent"].as_str()?)?,
            prompt: text(&value["prompt"]),
            when: When::from_json(&value["when"])?,
            enabled: value["enabled"] != false,
            anchor: int(&value["anchor"])?,
            last_run: Run::from_json(&value["lastRun"]),
        })
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "id": self.id, "name": self.name, "projectId": self.project_id, "agent": self.agent.id(), "prompt": self.prompt,
            "when": self.when.to_json(), "enabled": self.enabled, "anchor": self.anchor,
        });
        if let Some(run) = &self.last_run {
            out["lastRun"] = run.to_json();
        }
        out
    }

    pub fn draft(&self) -> Draft {
        Draft { name: self.name.clone(), project_id: self.project_id.clone(), agent: self.agent, prompt: self.prompt.clone(), when: self.when.clone() }
    }
}

impl Draft {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Draft {
            name: text(&value["name"]),
            project_id: text(&value["projectId"]),
            agent: Agent::from_id(value["agent"].as_str()?)?,
            prompt: text(&value["prompt"]),
            when: When::from_json(&value["when"])?,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "projectId": self.project_id, "agent": self.agent.id(), "prompt": self.prompt, "when": self.when.to_json() })
    }
}

pub const EVERY_HOURS: [i64; 8] = [1, 2, 3, 4, 6, 8, 12, 24];
pub const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// `Date.getDay()` for each of `DAYS`.
pub const DAY_NUMBERS: [i64; 7] = [1, 2, 3, 4, 5, 6, 0];
pub const MAX_PROMPT_BYTES: usize = 4000;

const HOUR: i64 = 3_600_000;
/// A run that starts later than this after its time was held up (the Mac slept), so the next one counts from now.
const LATE_MS: i64 = 120_000;

/// "HH:MM" as `[01]\d|2[0-3]` and `[0-5]\d`.
pub fn time_ok(time: &str) -> bool {
    let b = time.as_bytes();
    b.len() == 5 && b[2] == b':' && b[..2].iter().chain(&b[3..]).all(u8::is_ascii_digit) && b[0] <= b'2' && (b[0] != b'2' || b[1] <= b'3') && b[3] <= b'5'
}

fn clock_of(time: &str) -> (u32, u32) {
    let (hours, minutes) = time.split_once(':').unwrap_or_default();
    (hours.parse().unwrap_or(0), minutes.parse().unwrap_or(0))
}

/// When the schedule runs next, in ms, or none when no day is picked.
pub fn next_run(s: &Schedule) -> Option<i64> {
    let (time, days) = match &s.when {
        When::Every { hours } => return Some(s.anchor + hours * HOUR),
        When::Daily { time, days } => (time, days),
    };
    if days.is_empty() {
        return None;
    }
    let (hh, mm) = clock_of(time);
    let from = local(s.anchor);
    (0..=7).map(|i| at(from.year, from.month, i64::from(from.day) + i, hh, mm)).find(|moment| *moment > s.anchor && days.contains(&i64::from(local(*moment).weekday)))
}

/// The time a run is due, or none while the schedule is paused or not yet there.
pub fn due_at(s: &Schedule, now: i64) -> Option<i64> {
    if !s.enabled {
        return None;
    }
    next_run(s).filter(|at| *at <= now)
}

/// The schedule once the run due at `due` has been claimed: it can't be due at `due` again.
pub fn claim(s: &Schedule, due: i64, now: i64) -> Schedule {
    Schedule { anchor: if now - due < LATE_MS { due } else { now }, ..s.clone() }
}

/// On start, a schedule that came due while Wings was closed counts from now: nothing is made up.
pub fn on_start(s: &Schedule, now: i64) -> Schedule {
    if due_at(s, now).is_none() { s.clone() } else { Schedule { anchor: now, ..s.clone() } }
}

pub fn name_of(name: &str, prompt: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    if prompt.chars().count() > 48 { format!("{}…", prompt.chars().take(47).collect::<String>().trim_end()) } else { prompt.to_owned() }
}

impl Schedule {
    pub fn title(&self) -> String {
        name_of(&self.name, &self.prompt)
    }
}

/// The prompt as `openPane` takes it: one line, and nothing a shell or the agent's own flags could read. Fails with what the form shows.
pub fn clean_prompt(raw: &str) -> Result<String, String> {
    let mut one = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_control() && c.is_ascii() {
            one.truncate(one.trim_end_matches(char::is_whitespace).len());
            while chars.next_if(|next| next.is_whitespace() || (next.is_control() && next.is_ascii())).is_some() {}
            one.push(' ');
        } else {
            one.push(c);
        }
    }
    let prompt = one.trim();
    if prompt.is_empty() {
        return Err("Write what the agent should do.".into());
    }
    if prompt.starts_with('-') {
        return Err("The prompt can't start with a dash. Begin it with a word.".into());
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("The prompt is too long. Keep it under 4 KB.".into());
    }
    Ok(prompt.to_owned())
}

/// A draft that is safe to save, or the reason it isn't.
pub fn clean_draft(d: &Draft) -> Result<Draft, String> {
    if d.project_id.is_empty() {
        return Err("Pick a project.".into());
    }
    let when = match &d.when {
        When::Every { hours } => {
            if !EVERY_HOURS.contains(hours) {
                return Err("Pick how many hours.".into());
            }
            When::Every { hours: *hours }
        }
        When::Daily { time, days } => {
            if !time_ok(time) {
                return Err("Pick a time of day.".into());
            }
            let mut picked: Vec<i64> = Vec::new();
            for day in days {
                if DAY_NUMBERS.contains(day) && !picked.contains(day) {
                    picked.push(*day);
                }
            }
            picked.sort_by_key(|day| DAY_NUMBERS.iter().position(|n| n == day));
            if picked.is_empty() {
                return Err("Pick at least one day.".into());
            }
            When::Daily { time: time.clone(), days: picked }
        }
    };
    Ok(Draft { name: d.name.trim().chars().take(80).collect(), project_id: d.project_id.clone(), agent: d.agent, prompt: clean_prompt(&d.prompt)?, when })
}

pub fn describe_when(when: &When) -> String {
    let (time, days) = match when {
        When::Every { hours } => return if *hours == 1 { "Every hour".into() } else { format!("Every {hours} hours") },
        When::Daily { time, days } => (time, days),
    };
    let picked: Vec<i64> = DAY_NUMBERS.iter().copied().filter(|n| days.contains(n)).collect();
    let names = if picked.len() == 7 {
        "Every day".to_owned()
    } else if picked.len() == 5 && picked.iter().all(|n| (1..=5).contains(n)) {
        "Weekdays".to_owned()
    } else {
        picked.iter().map(|n| DAYS[DAY_NUMBERS.iter().position(|d| d == n).unwrap_or(0)]).collect::<Vec<_>>().join(", ")
    };
    format!("{names} at {time}")
}

/// "in 25 min", "in 3 h", "Today 18:00", "Tomorrow 09:00", "Mon 09:00".
pub fn describe_next(moment: i64, now: i64) -> String {
    let wait = moment - now;
    if wait < 60_000 {
        return "in under a minute".into();
    }
    if wait < HOUR {
        return format!("in {} min", (wait as f64 / 60_000.0).round());
    }
    if wait < 12 * HOUR {
        return format!("in {} h", (wait as f64 / HOUR as f64).round());
    }
    let d = local(moment);
    let days = ((start_of_day(moment) - start_of_day(now)) as f64 / (24 * HOUR) as f64).round() as i64;
    let clock = format!("{:02}:{:02}", d.hour, d.minute);
    match days {
        0 => format!("Today {clock}"),
        1 => format!("Tomorrow {clock}"),
        _ => format!("{} {clock}", DAYS[DAY_NUMBERS.iter().position(|n| *n == i64::from(d.weekday)).unwrap_or(0)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::local_time::testing;

    /// Summer time starts on Sunday 2026-03-29 here, so a day is 23 hours long once.
    fn zagreb() {
        let (start, end) = (at_utc(2026, 3, 29), at_utc(2026, 10, 25));
        testing::zone(move |ms| if (start as f64..end as f64).contains(&ms) { 120 } else { 60 });
    }

    fn at_utc(year: i64, month: u32, day: i64) -> i64 {
        // 01:00 UTC, as the clock goes forward and back.
        (days(year, month, day) * 24 + 1) * HOUR
    }

    fn days(year: i64, month: u32, day: i64) -> i64 {
        // Days since 1970 of a date in 2026, from the 4th of May's 20_577.
        let may4 = 20_577;
        let before: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        assert_eq!(year, 2026);
        may4 + (before[month as usize - 1] + day - 1) - (before[4] + 3)
    }

    fn t(y: i64, m: u32, d: i64, h: u32, min: u32) -> i64 {
        at(y, m, d, h, min)
    }

    fn make(when: When, anchor: i64) -> Schedule {
        Schedule { id: "a".into(), name: String::new(), project_id: "p".into(), agent: Agent::Claude, prompt: "Review".into(), when, enabled: true, anchor, last_run: None }
    }

    fn daily(time: &str, days: &[i64]) -> When {
        When::Daily { time: time.into(), days: days.to_vec() }
    }

    const ALL: [i64; 7] = [0, 1, 2, 3, 4, 5, 6];

    #[test]
    fn every_n_hours_counts_from_the_anchor_across_midnight() {
        zagreb();
        let s = make(When::Every { hours: 6 }, t(2026, 5, 4, 21, 15));
        assert_eq!(next_run(&s), Some(t(2026, 5, 5, 3, 15)));
        assert_eq!(due_at(&s, t(2026, 5, 5, 3, 14)), None);
        assert_eq!(due_at(&s, t(2026, 5, 5, 3, 15)), Some(t(2026, 5, 5, 3, 15)));
    }

    #[test]
    fn daily_runs_at_the_time_on_the_chosen_days_and_skips_the_others() {
        zagreb();
        // 2026-05-04 is a Monday. Only Mon and Thu: after Monday's 09:00 comes Thursday's.
        let s = make(daily("09:00", &[1, 4]), t(2026, 5, 4, 9, 0));
        assert_eq!(next_run(&s), Some(t(2026, 5, 7, 9, 0)));
        // Before today's time it runs today; after it, the next day picked.
        assert_eq!(next_run(&Schedule { anchor: t(2026, 5, 4, 8, 59), ..s.clone() }), Some(t(2026, 5, 4, 9, 0)));
        assert_eq!(next_run(&Schedule { anchor: t(2026, 5, 7, 9, 1), ..s }), Some(t(2026, 5, 11, 9, 0)));
        // Late evening to the next morning, over midnight.
        assert_eq!(next_run(&make(daily("00:30", &ALL), t(2026, 5, 4, 23, 50))), Some(t(2026, 5, 5, 0, 30)));
        assert_eq!(next_run(&make(daily("09:00", &[]), 0)), None);
    }

    #[test]
    fn daily_follows_the_clock_on_the_wall_through_the_change_to_summer_time() {
        zagreb();
        let s = make(daily("09:00", &ALL), t(2026, 3, 28, 9, 0));
        assert_eq!(next_run(&s), Some(t(2026, 3, 29, 9, 0)));
        assert_eq!(next_run(&s).unwrap() - s.anchor, 23 * HOUR);
        // 02:30 doesn't exist on the 29th: it runs once that day, not twice and not never.
        let skipped = next_run(&make(daily("02:30", &ALL), t(2026, 3, 28, 2, 30))).unwrap();
        assert_eq!(local(skipped).day, 29);
        assert_eq!(next_run(&make(daily("02:30", &ALL), skipped)), Some(t(2026, 3, 30, 2, 30)));
    }

    #[test]
    fn a_paused_schedule_is_never_due() {
        zagreb();
        let s = Schedule { enabled: false, ..make(When::Every { hours: 1 }, t(2026, 5, 4, 8, 0)) };
        assert_eq!(due_at(&s, t(2026, 5, 4, 20, 0)), None);
    }

    #[test]
    fn a_due_time_starts_once_claiming_it_moves_the_schedule_past_it() {
        zagreb();
        let s = make(When::Every { hours: 2 }, t(2026, 5, 4, 8, 0));
        let due = due_at(&s, t(2026, 5, 4, 10, 0) + 20_000).unwrap();
        assert_eq!(due, t(2026, 5, 4, 10, 0));
        let claimed = claim(&s, due, t(2026, 5, 4, 10, 0) + 20_000);
        // The next tick, and a second frame reading the claimed schedule, find nothing due.
        assert_eq!(due_at(&claimed, t(2026, 5, 4, 10, 0) + 50_000), None);
        // On time, the next run keeps the schedule's rhythm.
        assert_eq!(next_run(&claimed), Some(t(2026, 5, 4, 12, 0)));
        // Started late (the Mac slept), the next run counts from now instead of firing again at once.
        let late = claim(&s, due, t(2026, 5, 4, 15, 0));
        assert_eq!(due_at(&late, t(2026, 5, 4, 15, 1)), None);
        assert_eq!(next_run(&late), Some(t(2026, 5, 4, 17, 0)));
    }

    #[test]
    fn a_schedule_that_came_due_while_wings_was_closed_isnt_made_up() {
        zagreb();
        let s = make(daily("09:00", &ALL), t(2026, 5, 4, 9, 0));
        let now = t(2026, 5, 6, 12, 0);
        assert!(due_at(&s, now).is_some());
        let started = on_start(&s, now);
        assert_eq!(due_at(&started, now), None);
        assert_eq!(next_run(&started), Some(t(2026, 5, 7, 9, 0)));
        // One that isn't due yet keeps its time.
        let waiting = make(When::Every { hours: 12 }, t(2026, 5, 6, 8, 0));
        assert_eq!(on_start(&waiting, now), waiting);
    }

    #[test]
    fn a_prompt_is_one_line_that_cant_read_as_a_flag() {
        assert_eq!(clean_prompt("  Fix the\n\n  build\tplease ").unwrap(), "Fix the build please");
        assert!(clean_prompt("--resume").unwrap_err().contains("dash"));
        assert!(clean_prompt(" \n ").unwrap_err().starts_with("Write what"));
        assert!(clean_prompt(&"é".repeat(2001)).unwrap_err().contains("too long"));
    }

    #[test]
    fn a_draft_keeps_the_picked_days_in_the_weeks_order_and_refuses_what_the_form_names() {
        let draft = |when| Draft { name: "  Review  ".into(), project_id: "p".into(), agent: Agent::Claude, prompt: "Go".into(), when };
        let cleaned = clean_draft(&draft(daily("09:00", &[0, 5, 1, 1, 9]))).unwrap();
        assert_eq!((cleaned.name.as_str(), cleaned.when), ("Review", daily("09:00", &[1, 5, 0])));
        assert_eq!(clean_draft(&draft(daily("9:00", &[1]))).unwrap_err(), "Pick a time of day.");
        assert_eq!(clean_draft(&draft(daily("09:00", &[]))).unwrap_err(), "Pick at least one day.");
        assert_eq!(clean_draft(&draft(When::Every { hours: 5 })).unwrap_err(), "Pick how many hours.");
        assert!(["00:00", "23:59", "19:30"].iter().all(|time| time_ok(time)) && ["24:00", "09:60", "9:00", "09-00"].iter().all(|time| !time_ok(time)));
        assert_eq!([describe_when(&daily("09:00", &[1, 2, 3, 4, 5])), describe_when(&daily("09:00", &[6, 0])), describe_when(&When::Every { hours: 1 })], ["Weekdays at 09:00", "Sat, Sun at 09:00", "Every hour"]);
    }

    #[test]
    fn next_run_reads_as_a_wait_then_as_a_day_and_time() {
        zagreb();
        let now = t(2026, 5, 4, 10, 0);
        assert_eq!(describe_next(now + 25 * 60_000, now), "in 25 min");
        assert_eq!(describe_next(now + 3 * HOUR, now), "in 3 h");
        assert_eq!(describe_next(t(2026, 5, 4, 22, 0), now), "Today 22:00");
        assert_eq!(describe_next(t(2026, 5, 5, 9, 0), now), "Tomorrow 09:00");
        assert_eq!(describe_next(t(2026, 5, 11, 9, 0), now), "Mon 09:00");
    }
}
