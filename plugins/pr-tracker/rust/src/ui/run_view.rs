//! A check's run inside the Checks tab: its jobs and steps on the left, the picked step's log on the right, coloured and
//! folded the way the CI marked it. It asks for the run when it opens and when the check's state changes, and for a
//! step's log when the step is picked.

use crate::{
    checklist::duration_text,
    pr_page,
    runlog::{LogEntry, LogLine, Span, Tone, parse_log, pick_step},
    types::{CheckItem, CheckState, Run, RunStep, Status},
    ui::parts,
};
use std::{cell::RefCell, collections::HashSet};
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, button, list, list_row, muted, notice, row, stack, text},
};

/// A long log draws its end, where the error is, and "Show earlier" adds this many lines a click, up to `MOST_LINES`: the
/// native app takes a page of at most 20,000 nodes, and a line is a few of them.
const SHOWN_LINES: usize = 500;
const MOST_LINES: usize = 2_500;

enum Load<T> {
    Loading,
    Error(String),
    Done(T),
}

struct Log {
    entries: Vec<LogEntry>,
    /// Lines in all, a group's title counted with its lines.
    total: usize,
}

#[derive(Default)]
struct Ui {
    opened: Option<String>,
    run: Option<Load<Run>>,
    picked: Option<String>,
    /// The step the log is of, and how that step stood when it was read.
    log: Option<(String, Option<(CheckState, Option<f64>)>, Load<Log>)>,
    folded_open: HashSet<usize>,
    /// How many times "Show earlier" was pressed for the step shown.
    earlier: usize,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn ui<T>(change: impl FnOnce(&mut Ui) -> T) -> T {
    UI.with(|ui| change(&mut ui.borrow_mut()))
}

pub fn reset() {
    UI.with(|ui| *ui.borrow_mut() = Ui::default());
}

/// The check whose run shows in place of the list, when it is one of `items` and has a run.
pub fn opened_item(items: &[CheckItem]) -> Option<CheckItem> {
    let id = ui(|u| u.opened.clone())?;
    items.iter().find(|c| c.id == id && c.run.is_some()).cloned()
}

/// Whether the Checks tab shows a run, which widens the page so a log fits more of each line.
pub fn is_open(st: &Status) -> bool {
    opened_item(st.items.as_deref().unwrap_or_default()).is_some() && pr_page::read(|p| p.can.run)
}

pub fn close() {
    ui(|u| *u = Ui::default());
}

/// Shows the run of the check `id`.
pub fn open(id: &str) {
    ui(|u| *u = Ui { opened: Some(id.to_owned()), ..Ui::default() });
    load_run();
}

fn current_item() -> Option<CheckItem> {
    let id = ui(|u| u.opened.clone())?;
    pr_page::status()?.items?.into_iter().find(|c| c.id == id)
}

/// Reads the run again, keeping what is drawn until the new answer comes.
fn load_run() {
    let Some(item) = current_item() else { return };
    let Some(run_ref) = item.run.clone() else { return };
    ui(|u| {
        if !matches!(u.run, Some(Load::Done(_))) {
            u.run = Some(Load::Loading);
        }
    });
    spawn(async move {
        let load = match pr_page::load_run(run_ref).await {
            Ok(run) => Load::Done(run),
            Err(message) => Load::Error(message),
        };
        // The check was closed or another one opened meanwhile.
        if ui(|u| u.opened.as_deref() != Some(item.id.as_str())) {
            return;
        }
        ui(|u| u.run = Some(load));
        sync_log(false);
    });
}

/// Reads the shown step's log when it is another step than the one read, or, with `force`, when it stood otherwise.
fn sync_log(force: bool) {
    let Some(item) = current_item() else { return };
    let Some(run_ref) = item.run.clone() else { return };
    let Some(step) = ui(|u| match &u.run {
        Some(Load::Done(run)) => current_step_in(run, u.picked.as_deref()),
        _ => None,
    }) else {
        return;
    };
    if step.log.is_none() {
        return ui(|u| u.log = None);
    }
    let stood = Some((step.state, step.seconds));
    let same = ui(|u| u.log.as_ref().is_some_and(|(id, was, _)| *id == step.id && (!force || *was == stood)));
    if same {
        return;
    }
    // A reload of the same step keeps its lines until the new ones come, so a running log doesn't blink.
    ui(|u| {
        if !matches!(&u.log, Some((id, _, Load::Done(_))) if *id == step.id) {
            u.log = Some((step.id.clone(), stood, Load::Loading));
        }
    });
    spawn(async move {
        let load = match pr_page::load_run_log(run_ref, step.clone()).await {
            Ok(lines) => {
                let entries = parse_log(&lines);
                let total = entries.iter().map(|e| match e {
                    LogEntry::Group { lines, .. } => lines.len() + 1,
                    LogEntry::Line(_) => 1,
                }).sum();
                Load::Done(Log { entries, total })
            }
            Err(message) => Load::Error(message),
        };
        // Another step was picked meanwhile.
        ui(|u| {
            if u.log.as_ref().is_some_and(|(id, _, _)| *id == step.id) {
                u.log = Some((step.id.clone(), Some((step.state, step.seconds)), load));
            }
        });
    });
}

/// The PR's status was read again: a run that is going is read again with it.
pub fn status_loaded(st: &Status) {
    let Some(id) = ui(|u| u.opened.clone()) else { return };
    let Some(item) = st.items.as_deref().unwrap_or_default().iter().find(|c| c.id == id) else { return };
    if matches!(item.state, CheckState::Running | CheckState::Queued) {
        load_run();
    }
}

fn refresh() {
    load_run();
    sync_log(true);
}

fn pick(id: String) {
    ui(|u| {
        u.picked = Some(id);
        u.earlier = 0;
    });
    sync_log(false);
}

// ---------- drawing ----------

pub fn build(item: &CheckItem, _st: &Status) -> Node {
    let going = matches!(item.state, CheckState::Running | CheckState::Queued);
    let (run, picked_log_loading) = ui(|u| (u.run.as_ref().map(|r| match r {
        Load::Loading => 0,
        Load::Error(_) => 1,
        Load::Done(_) => 2,
    }), matches!(&u.log, Some((_, _, Load::Loading)))));
    let busy = run == Some(0) || picked_log_loading;
    let run_url = ui(|u| match &u.run {
        Some(Load::Done(run)) => run.url.clone(),
        _ => None,
    });
    let url = run_url.or_else(|| item.url.clone()).filter(|url| url.starts_with("https://"));
    let head = row()
        .prop("gap", "s")
        .child(Node::new("Button").prop("variant", "ghost").prop("size", "sm").on("onClick", |_| close()).text("← Checks"))
        .child(parts::state_mark(item.state))
        .child(text(item.name.as_str()).prop("weight", "strong").prop("truncate", true))
        .maybe(duration_text(item.seconds).map(|d| muted(d).prop("size", "small")))
        .child(
            row()
                .prop("gap", "s")
                .prop("grow", true)
                .prop("justify", "end")
                .child({
                    let button = Node::new("Button").prop("variant", "outline").prop("size", "sm").on("onClick", |_| refresh());
                    if busy { button.prop("disabled", true).child(wings_plugin::ui::spinner()).text("Refresh") } else { button.text("Refresh") }
                })
                .maybe(url.map(|url| Node::new("Button").prop("variant", "outline").prop("size", "sm").on("onClick", move |_| host::open_url(&url)).text("Open"))),
        );
    let body = ui(|u| match &u.run {
        None | Some(Load::Loading) => vec![Node::new("Loading").text("Loading the run…")],
        Some(Load::Error(message)) => vec![notice("danger", message.as_str())],
        Some(Load::Done(run)) => {
            let mut out = Vec::new();
            if run.jobs.is_empty() {
                out.push(muted(if going { "The run hasn't started any jobs yet." } else { "This run has no jobs." }).prop("size", "small"));
            }
            if let Some(note) = &run.note {
                out.push(muted(note.as_str()).prop("size", "small"));
            }
            if !run.jobs.is_empty() {
                let step = current_step_in(run, u.picked.as_deref());
                let current = step.as_ref().map(|s| s.id.clone());
                let detail = match &step {
                    Some(step) => step_log(step, u),
                    None => muted("Pick a step to see its log.").prop("size", "small"),
                };
                out.push(row().prop("gap", "l").prop("align", "start").child(stack().prop("width", 300).child(steps(run, current.as_deref()))).child(stack().prop("grow", true).child(detail)));
            }
            out
        }
    });
    stack().prop("gap", "m").child(head).children(body)
}

/// The steps a run lists, and the one shown: the one picked, else the one a failure or the end of the run points at.
fn current_step_in(run: &Run, picked: Option<&str>) -> Option<RunStep> {
    let steps: Vec<RunStep> = run.jobs.iter().flat_map(|j| j.steps.clone()).collect();
    picked.and_then(|id| steps.iter().find(|s| s.id == id).cloned()).or_else(|| {
        let with_log: Vec<RunStep> = steps.iter().filter(|s| s.log.is_some()).cloned().collect();
        pick_step(&with_log).cloned()
    })
}

fn steps(run: &Run, current: Option<&str>) -> Node {
    let mut stage: Option<&str> = None;
    let mut out: Vec<Node> = Vec::new();
    for j in &run.jobs {
        let heading = j.stage.as_deref().filter(|s| Some(*s) != stage);
        stage = j.stage.as_deref();
        if let Some(heading) = heading {
            out.push(muted(heading).prop("size", "tiny").key(format!("stage-{heading}")));
        }
        let steps = if j.steps.is_empty() {
            muted(if j.state == CheckState::Queued { "Waiting to start" } else { "No steps" }).prop("size", "tiny")
        } else {
            list().children(j.steps.iter().map(|s| {
                let id = s.id.clone();
                let mut button = Node::new("ListButton").on("onClick", move |_| pick(id.clone()));
                if s.log.is_none() {
                    button = button.prop("disabled", true);
                }
                list_row(
                    s.id.clone(),
                    button
                        .child(parts::state_mark(s.state))
                        .child(text(s.name.as_str()).prop("grow", true).prop("truncate", true).prop("weight", if current == Some(s.id.as_str()) { "strong" } else { "normal" }))
                        .maybe(duration_text(s.seconds).map(|d| muted(d).prop("size", "tiny"))),
                )
            }))
        };
        out.push(
            stack()
                .prop("gap", "xs")
                .key(j.id.clone())
                .child(row().prop("gap", "s").child(parts::state_mark(j.state)).child(text(j.name.as_str()).prop("weight", "strong").prop("grow", true)).maybe(duration_text(j.seconds).map(|d| muted(d).prop("size", "tiny"))))
                .child(steps),
        );
    }
    stack().prop("gap", "m").children(out)
}

fn line_tone(tone: Tone) -> &'static str {
    match tone {
        Tone::Error => "danger",
        Tone::Warning => "warn",
        Tone::Command | Tone::Section => "info",
        Tone::Debug => "muted",
    }
}

fn fg_tone(fg: u8) -> Option<&'static str> {
    match fg {
        0 | 8 => Some("muted"),
        1 | 9 => Some("danger"),
        2 | 10 => Some("success"),
        3 | 11 => Some("warn"),
        4..=6 | 12..=14 => Some("info"),
        _ => None,
    }
}

/// A line's text with the looks the CI gave its runs.
fn spans(spans: &[Span], text_: Node) -> Node {
    spans.iter().fold(text_, |node, s| {
        let plain = s.style.fg.is_none() && !s.style.bold && !s.style.dim && !s.style.italic && !s.style.underline;
        if plain {
            return node.text(s.text.as_str());
        }
        let mut part = Node::new("Text").text(s.text.as_str());
        let tone = if s.style.dim { Some("muted") } else { s.style.fg.and_then(fg_tone) };
        if let Some(tone) = tone {
            part = part.prop("tone", tone);
        }
        if s.style.bold {
            part = part.prop("weight", "strong");
        }
        node.child(part)
    })
}

fn number(n: usize, digits: usize) -> Node {
    muted(format!("{n:>digits$}")).prop("mono", true).prop("size", "tiny")
}

fn log_line(l: &LogLine, digits: usize) -> Node {
    let mut code = Node::new("Text").prop("mono", true).prop("size", "small").prop("grow", true);
    if let Some(tone) = l.tone {
        code = code.prop("tone", line_tone(tone));
    }
    row().prop("gap", "m").prop("align", "baseline").key(l.n.to_string()).child(number(l.n, digits)).child(spans(&l.spans, code))
}

/// The end of a log, `max` lines, keeping a group whole once any of it is in.
fn last_lines(entries: &[LogEntry], max: usize) -> &[LogEntry] {
    let (mut n, mut i) = (0, entries.len());
    while i > 0 && n < max {
        i -= 1;
        n += match &entries[i] {
            LogEntry::Group { lines, .. } => lines.len() + 1,
            LogEntry::Line(_) => 1,
        };
    }
    &entries[i..]
}

fn step_log(step: &RunStep, u: &Ui) -> Node {
    let log = u.log.as_ref().filter(|(id, _, _)| *id == step.id).map(|(_, _, l)| l);
    let log = match log {
        None | Some(Load::Loading) => return Node::new("Loading").text("Loading the log…"),
        Some(Load::Error(message)) => return notice("danger", message.as_str()),
        Some(Load::Done(log)) => log,
    };
    if log.entries.is_empty() {
        return muted(if step.state == CheckState::Running { "No output yet." } else { "This step wrote nothing." }).prop("size", "small");
    }
    let room = (SHOWN_LINES * (1 + u.earlier)).min(MOST_LINES);
    let shown = last_lines(&log.entries, room);
    let last = match shown.last() {
        Some(LogEntry::Group { title, lines }) => lines.last().unwrap_or(title).n,
        Some(LogEntry::Line(l)) => l.n,
        None => 0,
    };
    let digits = last.to_string().len();
    let lines = shown.iter().map(|e| match e {
        LogEntry::Line(l) => log_line(l, digits),
        LogEntry::Group { title, lines } => {
            let n = title.n;
            let open = u.folded_open.contains(&n);
            let head = Node::new("Text").prop("mono", true).prop("size", "small").prop("grow", true).prop("weight", "strong");
            stack()
                .prop("gap", "none")
                .key(format!("g{n}"))
                .child(
                    row()
                        .prop("gap", "m")
                        .prop("align", "baseline")
                        .child(number(n, digits))
                        .child(Node::new("ListButton").prop("align", "baseline").prop("aria-expanded", open).on("onClick", move |_| ui(|u| {
                            if !u.folded_open.remove(&n) {
                                u.folded_open.insert(n);
                            }
                        })).child(spans(&title.spans, head))),
                )
                .children(open.then(|| lines.iter().map(|l| log_line(l, digits))).into_iter().flatten())
        }
    });
    let cut = shown.len() != log.entries.len();
    let earlier = cut.then(|| {
        let more = SHOWN_LINES.min(MOST_LINES - room).min(log.total.saturating_sub(room));
        let said = format!("Showing the last {room} of {} lines.", log.total);
        let said = if more == 0 { format!("{said} Open the run on the CI's page to read the rest.") } else { said };
        row()
            .prop("gap", "s")
            .prop("align", "center")
            .maybe((more > 0).then(|| button(format!("Show {more} earlier lines"), "ghost", false, show_earlier).prop("size", "sm")))
            .child(muted(said).prop("size", "tiny"))
    });
    stack().prop("gap", "xs").maybe(earlier).child(stack().prop("gap", "none").children(lines))
}

/// "Show earlier": the log of the shown step draws another page of lines before the ones it draws.
fn show_earlier() {
    ui(|u| u.earlier += 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runlog::Style;
    use wings_plugin::ui::tree_json;

    fn step() -> RunStep {
        RunStep { id: "s1".into(), name: "Build".into(), state: CheckState::Failed, seconds: None, log: Some("l".into()) }
    }

    fn with_log(total: usize) {
        let entries = (1..=total).map(|n| LogEntry::Line(LogLine { n, time: None, tone: None, spans: vec![Span { text: format!("line {n}"), style: Style::default() }] })).collect();
        ui(|u| *u = Ui { log: Some(("s1".into(), None, Load::Done(Log { entries, total }))), ..Ui::default() });
    }

    fn drawn() -> String {
        tree_json(ui(|u| step_log(&step(), u))).to_string()
    }

    /// A log of 1,300 lines drew its last 500 and no way to the rest; now each press draws 500 more before them, up to the
    /// whole log, and the last lines stay where they were.
    #[test]
    fn show_earlier_draws_another_page_of_lines_before_the_ones_shown() {
        with_log(1_300);
        let first = drawn();
        assert!(first.contains("line 1300") && first.contains("line 801") && !first.contains("\"line 800\""), "the last 500");
        assert!(first.contains("Show 500 earlier lines") && first.contains("Showing the last 500 of 1300 lines."));
        show_earlier();
        let second = drawn();
        assert!(second.contains("line 301") && !second.contains("\"line 300\"") && second.contains("line 1300"));
        show_earlier();
        let all = drawn();
        assert!(all.contains("line 1\"") && all.contains("line 1300") && !all.contains("earlier lines") && !all.contains("Showing the last"), "{all}");
        // The last press offers only what is left.
        with_log(1_300);
        show_earlier();
        assert!(drawn().contains("Show 300 earlier lines"));
    }

    #[test]
    fn a_very_long_log_stops_at_what_a_page_holds_and_says_where_to_read_the_rest() {
        with_log(10_000);
        (0..10).for_each(|_| show_earlier());
        let most = drawn();
        assert!(most.contains("line 10000") && most.contains("line 7501") && !most.contains("\"line 7500\""));
        assert!(!most.contains("earlier lines") && most.contains("Showing the last 2500 of 10000 lines. Open the run"));
        // Another step starts at the end again.
        pick("s2".into());
        assert_eq!(ui(|u| u.earlier), 0);
    }
}
