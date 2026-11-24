//! The side panel's group of what this chat's Claude scheduled: each task and the loop's pending wakeup, when it runs next,
//! and a Stop button that asks Claude to cancel it.

use wings_plugin::{
    format::clock,
    ui::{Entry, Node, button, entry, icon, icons, muted, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    scheduled::Job,
};

/// Lucide's `clock` for a one-time task or a wakeup, in a 24-point box.
const CLOCK: [&str; 2] = ["M12 6v6l4 2", "M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0"];

/// A one-time task or wakeup keeps showing this long past its time, as Claude Code may run it a little late.
const LATE: u64 = 120_000;

/// None when nothing is scheduled.
pub fn section(c: &Chat) -> Option<Node> {
    let now = c.session.now;
    let jobs: Vec<&Job> = c.session.scheduled.jobs.iter().filter(|job| job.recurring || job.next == 0 || job.next + LATE > now).collect();
    if jobs.is_empty() {
        return None;
    }
    Some(crate::view::fold(c, "scheduled", format!("Scheduled · {}", jobs.len()), jobs.into_iter().map(row_of)))
}

fn row_of(job: &Job) -> Node {
    let next = match job.next {
        0 => String::new(),
        ms => format!(" · next {}", clock(ms as f64)),
    };
    let kind = match (job.wakeup, job.recurring) {
        (true, _) => "",
        (false, true) => " · recurring",
        (false, false) => " · once",
    };
    let detail = if job.stopping { "Stopping".to_owned() } else { format!("{}{kind}{next}", job.when) };
    let main = entry(Entry {
        lead: Some(icon(24, 2.0, if job.recurring { &icons::ROTATE_CW[..] } else { &CLOCK[..] }).prop("size", 14)),
        title: text(job.what.as_str()).prop("size", "small").prop("clamp", true).prop("title", job.what.as_str()),
        detail: Some(muted(detail).prop("size", "small")),
        trailing: None,
    });
    let id = job.id.clone();
    let stop = button("", "ghost", job.stopping, move || chat::stop_scheduled(&id))
        .prop("size", "icon-sm")
        .prop("label", "Stop")
        .prop("title", "Ask Claude to stop this")
        .child(icon(14, 2.0, &[crate::tasks_view::STOP]));
    row().prop("gap", "xs").prop("align", "center").child(stack().prop("grow", true).child(main)).child(stop).key(job.id.as_str())
}

#[cfg(test)]
mod tests {
    use wings_plugin::ui::tree_json;

    use super::*;

    fn job(id: &str, wakeup: bool, recurring: bool, next: u64) -> Job {
        Job { id: id.into(), what: format!("run {id}"), when: "Every 10 minutes".into(), recurring, next, wakeup, stopping: false }
    }

    #[test]
    fn the_group_lists_tasks_and_the_wakeup_and_leaves_out_a_once_that_has_run() {
        chat::with(|c| {
            c.session.now = 10_000_000;
            c.session.scheduled.jobs = vec![job("a", false, true, 10_600_000), job("w", true, false, 10_300_000), job("old", false, false, 1_000_000)];
            let drawn = tree_json(section(c).expect("a group")).to_string();
            assert!(drawn.contains("Scheduled · 2") && drawn.contains("run a") && drawn.contains("run w") && !drawn.contains("run old"), "{drawn}");
            assert_eq!(drawn.matches("\"label\":\"Stop\"").count(), 2, "{drawn}");
            c.session.scheduled.jobs.clear();
            assert!(section(c).is_none());
        });
    }
}
