//! What the `list` tool prints: plain text, one line for each entry, ticket and waiting ticket.

use crate::{
    assigned::{Watch, describe_check, started_elsewhere},
    schedule::{Schedule, describe_when},
};

pub fn format_list(schedules: &[Schedule], watches: &[Watch], project_name: impl Fn(&str) -> String, now: i64) -> String {
    let mut lines: Vec<String> = Vec::new();
    let state = |enabled: bool| if enabled { "enabled" } else { "paused" };
    for s in schedules {
        lines.push(format!("{} · schedule · {} · {} · {} · {} · {}", s.id, project_name(&s.project_id), s.agent.id(), state(s.enabled), s.title(), describe_when(&s.when)));
    }
    for w in watches {
        let checked = match &w.last_check {
            Some(check) => format!("checked {} min ago: {}", ((now - check.at) as f64 / 60_000.0).round().max(0.0), describe_check(Some(check), w.source.as_ref())),
            None => describe_check(None, w.source.as_ref()),
        };
        let which = match &w.states {
            None => "states not picked yet".to_owned(),
            Some(states) if states.is_empty() => "asks about every state".to_owned(),
            Some(states) => format!("asks about {}", states.join(", ")),
        };
        let titles = match &w.prefixes {
            Some(prefixes) if !prefixes.is_empty() => format!(" · only titles starting with {}", prefixes.join(", ")),
            _ => String::new(),
        };
        let source = w.source.as_ref().map_or("source not read yet", |source| source.label());
        lines.push(format!("{} · assigned · {} · {} · {} · {source} · {checked} · {which}{titles}", w.id, project_name(&w.project_id), w.agent.id(), state(w.enabled)));
        for t in w.tickets.iter().flatten() {
            let kind = t.kind.as_ref().map_or(String::new(), |kind| format!("{kind} · "));
            let elsewhere = started_elsewhere(t).map_or(String::new(), |started| format!(" · started: {started}"));
            let here = if w.started.iter().any(|(id, _)| *id == t.ticket.id) { " · started here" } else { "" };
            lines.push(format!("  #{} · {} · {kind}{} · {}{elsewhere}{here}", t.ticket.id, t.ticket.title, t.state, if w.asks(t) { "asked about" } else { "not asked about" }));
        }
    }
    for w in watches {
        for t in &w.waiting {
            lines.push(format!("waiting · {} · #{} · {} · {}", w.id, t.ticket.id, t.ticket.title, t.ticket.url));
        }
    }
    if lines.is_empty() { "Nothing is set up yet.".into() } else { lines.join("\n") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_list_has_a_line_for_each_entry_ticket_and_waiting_ticket() {
        let schedule = Schedule::from_json(&json!({ "id": "a1", "name": "", "projectId": "p", "agent": "opencode", "prompt": "Check CI", "when": { "kind": "every", "hours": 6 }, "enabled": false, "anchor": 0 })).unwrap();
        let watch = Watch::from_json(&json!({
            "id": "w1", "projectId": "p", "agent": "claude", "prompt": "x", "minFreeGb": 10, "enabled": true, "primed": true, "seen": [], "states": ["Open"], "prefixes": ["FE"], "started": { "42": 1 },
            "waiting": [{ "id": "42", "title": "FE: coupon", "url": "https://x.test/42", "at": 1 }],
            "source": { "kind": "github", "host": "github.com", "path": "a/b" }, "lastCheck": { "at": 0, "ok": true, "count": 2 },
            "tickets": [{ "id": "42", "title": "FE: coupon", "url": "https://x.test/42", "state": "Open", "type": "bug" }, { "id": "40", "title": "BE: rates", "url": "https://x.test/40", "state": "In review", "branch": { "name": "feature/40-rates", "project": "shop" } }],
        }))
        .unwrap();
        let text = format_list(&[schedule], &[watch], |id| format!("project {id}"), 5 * 60_000);
        assert_eq!(
            text,
            "a1 · schedule · project p · opencode · paused · Check CI · Every 6 hours\n\
             w1 · assigned · project p · claude · enabled · GitHub · checked 5 min ago: 2 assigned to you · asks about Open · only titles starting with FE\n  \
             #42 · FE: coupon · bug · Open · asked about · started here\n  \
             #40 · BE: rates · In review · not asked about · started: feature/40-rates (shop)\n\
             waiting · w1 · #42 · FE: coupon · https://x.test/42"
        );
        assert_eq!(format_list(&[], &[], |_| String::new(), 0), "Nothing is set up yet.");
    }
}
