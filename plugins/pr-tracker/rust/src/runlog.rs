//! A CI log as the run view draws it: colours, the CI's markup and folded groups.

use crate::checklist::iso_to_ms;
use crate::types::{CheckState, RunStep};

/// What the CI marked a line as, by its `##[...]` prefix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Error,
    Warning,
    Command,
    Section,
    Debug,
}

/// How a run of text is styled. `fg` is an ANSI colour, 0 to 15.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Style {
    pub fg: Option<u8>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A run of text in one style.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogLine {
    pub n: usize,
    /// HH:MM:SS, where the CI stamped the line.
    pub time: Option<String>,
    pub tone: Option<Tone>,
    pub spans: Vec<Span>,
}

/// A line, or a group the CI folds (`##[group]`, GitLab's sections) with its title line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogEntry {
    Line(LogLine),
    Group { title: LogLine, lines: Vec<LogLine> },
}

/// A leading `2026-10-16T17:03:38.45Z ` stamp: the time as a whole, its HH:MM:SS, and how many bytes it takes.
fn stamp(text: &str) -> Option<(&str, &str, usize)> {
    let b = text.as_bytes();
    let digits = |from: usize, n: usize| b.get(from..from + n).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let shape = digits(0, 4) && b.get(4) == Some(&b'-') && digits(5, 2) && b.get(7) == Some(&b'-') && digits(8, 2) && b.get(10) == Some(&b'T') && digits(11, 2) && b.get(13) == Some(&b':') && digits(14, 2) && b.get(16) == Some(&b':') && digits(17, 2);
    if !shape {
        return None;
    }
    let mut end = 19;
    while b.get(end).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
        end += 1;
    }
    if b.get(end) != Some(&b'Z') {
        return None;
    }
    let whole = &text[..=end];
    let used = if b.get(end + 1) == Some(&b' ') { end + 2 } else { end + 1 };
    Some((whole, &text[11..19], used))
}

/// An escape sequence `ESC [ params letter` at the start of `text`: its params, its letter and its length.
fn escape(text: &str) -> Option<(&str, char, usize)> {
    let rest = text.strip_prefix("\u{1b}[")?;
    let params = rest.find(|c: char| !(c.is_ascii_digit() || c == ';' || c == '?'))?;
    let letter = rest[params..].chars().next().filter(char::is_ascii_alphabetic)?;
    Some((&rest[..params], letter, 2 + params + 1))
}

fn apply_sgr(style: Style, codes: &[i64]) -> Style {
    let mut s = style;
    let mut i = 0;
    while i < codes.len() {
        let c = codes[i];
        match c {
            0 => s = Style::default(),
            1 => s.bold = true,
            2 => s.dim = true,
            3 => s.italic = true,
            4 => s.underline = true,
            22 => {
                s.bold = false;
                s.dim = false;
            }
            23 => s.italic = false,
            24 => s.underline = false,
            39 => s.fg = None,
            30..=37 => s.fg = Some((c - 30) as u8),
            90..=97 => s.fg = Some((c - 82) as u8),
            38 | 48 => {
                // 256 colours keep their first 16; true colour is dropped.
                match codes.get(i + 1) {
                    Some(5) => {
                        if c == 38 && codes.get(i + 2).is_some_and(|n| (0..16).contains(n)) {
                            s.fg = Some(codes[i + 2] as u8);
                        }
                        i += 2;
                    }
                    Some(2) => i += 4,
                    _ => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
    s
}

/// The text of a line in styled runs. `style` carries over from the line before, as a terminal's does.
pub fn spans_of(text: &str, style: Style) -> (Vec<Span>, Style) {
    let mut spans = Vec::new();
    let mut style = style;
    let mut at = 0;
    let mut from = 0;
    let push = |spans: &mut Vec<Span>, t: &str, style: Style| {
        if !t.is_empty() {
            spans.push(Span { text: t.to_owned(), style });
        }
    };
    while let Some(found) = text[from..].find('\u{1b}') {
        let start = from + found;
        match escape(&text[start..]) {
            Some((params, letter, len)) => {
                push(&mut spans, &text[at..start], style);
                at = start + len;
                from = at;
                if letter == 'm' {
                    let codes: Vec<i64> = if params.is_empty() { vec![0] } else { params.split(';').map(|p| if p.is_empty() { 0 } else { p.parse().unwrap_or(-1) }).collect() };
                    style = apply_sgr(style, &codes);
                }
            }
            None => from = start + 1,
        }
    }
    push(&mut spans, &text[at..], style);
    (spans, style)
}

/// GitLab's section markers, which can follow each other on one line, taken out of `text`: what is left, and for each
/// whether it started a section (`true`) or ended one.
fn take_sections(text: &str) -> (String, Vec<bool>) {
    let mut out = String::new();
    let mut kinds = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("section_") {
        let after = &rest[at + "section_".len()..];
        let matched = (|| {
            let (start, tail) = if let Some(t) = after.strip_prefix("start:") { (true, t) } else { (false, after.strip_prefix("end:")?) };
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            let tail = tail[digits..].strip_prefix(':').filter(|_| digits > 0)?;
            let name = tail.find(['\r', '\n', '[']).unwrap_or(tail.len());
            if name == 0 {
                return None;
            }
            let mut tail = &tail[name..];
            if tail.starts_with('[') && let Some(close) = tail.find(']') {
                tail = &tail[close + 1..];
            }
            let tail = tail.strip_prefix('\r').unwrap_or(tail);
            let tail = tail.strip_prefix("\u{1b}[0K").unwrap_or(tail);
            Some((start, tail))
        })();
        match matched {
            Some((start, tail)) => {
                out.push_str(&rest[..at]);
                kinds.push(start);
                rest = tail;
            }
            None => {
                out.push_str(&rest[..at + "section_".len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    (out, kinds)
}

const MARKS: [&str; 7] = ["group", "endgroup", "section", "command", "error", "warning", "debug"];

/// The marker a line starts with, `[error]` or `##[error]`, and how long it is.
fn mark(line: &str) -> Option<(&'static str, usize)> {
    let rest = line.strip_prefix("##").unwrap_or(line);
    let skipped = line.len() - rest.len();
    let inner = rest.strip_prefix('[')?;
    MARKS.iter().find_map(|name| inner.strip_prefix(name).and_then(|t| t.strip_prefix(']')).map(|_| (*name, skipped + name.len() + 2)))
}

/// Takes a leading `##[word]` off `line`.
pub(crate) fn without_markup(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("##[") && let Some(end) = rest.find(']') && end > 0 && rest[..end].chars().all(|c| c.is_alphanumeric() || c == '_') {
        return &rest[end + 1..];
    }
    line
}

/// A log's lines as entries: timestamps lifted out, the CI's markup read, progress redraws collapsed to what was left.
pub fn parse_log(raw: &[String]) -> Vec<LogEntry> {
    let mut out = Vec::new();
    let mut group: Option<(LogLine, Vec<LogLine>)> = None;
    let mut style = Style::default();
    let mut n = 0;
    fn close(out: &mut Vec<LogEntry>, group: &mut Option<(LogLine, Vec<LogLine>)>) {
        if let Some((title, lines)) = group.take() {
            out.push(LogEntry::Group { title, lines });
        }
    }
    for source in raw {
        let mut text = source.as_str();
        let mut time = None;
        if let Some((_, hms, used)) = stamp(text) {
            time = Some(hms.to_owned());
            text = &text[used..];
        }
        // GitLab: a section's end closes its group, a start opens one titled by the rest of the line.
        let (text, kinds) = take_sections(text);
        let marked = !kinds.is_empty();
        let mut starts = false;
        for start in kinds {
            if start {
                starts = true;
            } else {
                close(&mut out, &mut group);
            }
        }
        let last = text.split('\r').rfind(|part| !part.is_empty()).unwrap_or("");
        let found = mark(last);
        let body = match found {
            Some((_, len)) => &last[len..],
            None => without_markup(last),
        };
        let marker = found.map(|(name, _)| name);
        if marker == Some("endgroup") {
            close(&mut out, &mut group);
            continue;
        }
        let tone = match marker {
            Some("section") => Some(Tone::Section),
            Some("command") => Some(Tone::Command),
            Some("error") => Some(Tone::Error),
            Some("warning") => Some(Tone::Warning),
            Some("debug") => Some(Tone::Debug),
            _ => None,
        };
        let (spans, next) = spans_of(body, style);
        style = next;
        if marked && !starts && body.trim().is_empty() {
            continue;
        }
        n += 1;
        let line = LogLine { n, time, tone, spans };
        if marker == Some("group") || starts {
            close(&mut out, &mut group);
            group = Some((line, Vec::new()));
        } else if let Some((_, lines)) = group.as_mut() {
            lines.push(line);
        } else {
            out.push(LogEntry::Line(line));
        }
    }
    close(&mut out, &mut group);
    out
}

/// Where the Actions runner starts a step's output: a `run` or `uses` step's group, a post step, and the job's own cleanup.
fn step_start(text: &str) -> bool {
    text.starts_with("##[group]Run ") || text.starts_with("Post job cleanup.") || text.starts_with("Cleaning up orphan processes")
}

/// An Actions job's log cut into its steps, which start and end to the second only, so several can share one. A line
/// goes to the next step once it is stamped after the current step ended, or when it is where the runner starts a
/// step and the next step has started by then. An unstamped line goes with the one before. `steps` are each step's
/// start and end time.
pub fn step_lines(raw: &[String], steps: &[(Option<String>, Option<String>)]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = steps.iter().map(|_| Vec::new()).collect();
    let sec = |iso: &Option<String>| iso.as_deref().and_then(iso_to_ms).map_or(f64::INFINITY, |ms| (ms / 1000.0).floor());
    let mut i = 0;
    for line in raw {
        if let Some((whole, _, used)) = stamp(line) && !steps.is_empty() {
            let t = (iso_to_ms(whole).unwrap_or(0.0) / 1000.0).floor();
            let text = &line[used..];
            while i + 1 < steps.len() && sec(&steps[i + 1].0) <= t && (t > sec(&steps[i].1) || (step_start(text) && !out[i].is_empty())) {
                i += 1;
            }
        }
        if let Some(part) = out.get_mut(i) {
            part.push(line.clone());
        }
    }
    out
}

/// The step a run view opens on: the first that failed, else the one running, else the longest.
pub fn pick_step(steps: &[RunStep]) -> Option<&RunStep> {
    steps
        .iter()
        .find(|s| s.state == CheckState::Failed)
        .or_else(|| steps.iter().find(|s| s.state == CheckState::Running))
        .or_else(|| steps.iter().fold(None, |a: Option<&RunStep>, s| if s.seconds.unwrap_or(-1.0) > a.and_then(|a| a.seconds).unwrap_or(-1.0) { Some(s) } else { a }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &LogLine) -> String {
        line.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parse_log_lifts_timestamps_reads_the_cis_markers_and_folds_its_groups() {
        let out = parse_log(&lines(&[
            "2026-10-16T17:03:38.4518137Z ##[section]Starting: Lint",
            "2026-10-16T17:03:38.5688826Z [command]/usr/bin/bash run.sh",
            "2026-10-16T17:03:39.0000000Z ##[group]Install",
            "2026-10-16T17:03:39.1000000Z Done in 4s",
            "2026-10-16T17:03:39.2000000Z ##[endgroup]",
            "2026-10-16T17:03:40.0000000Z ##[error]Bash exited with code '1'.",
        ]));
        let shown: Vec<String> = out
            .iter()
            .map(|e| match e {
                LogEntry::Group { title, lines } => format!("group {}: {}", text(title), lines.iter().map(text).collect::<Vec<_>>().join(",")),
                LogEntry::Line(l) => format!("{:?} {}", l.tone, text(l)),
            })
            .collect();
        assert_eq!(shown, ["Some(Section) Starting: Lint", "Some(Command) /usr/bin/bash run.sh", "group Install: Done in 4s", "Some(Error) Bash exited with code '1'."]);
        assert!(matches!(&out[0], LogEntry::Line(l) if l.time.as_deref() == Some("17:03:38")));
    }

    #[test]
    fn parse_log_keeps_ansi_colours_as_styles_carried_over_to_the_next_line_until_reset() {
        let out = parse_log(&lines(&["\u{1b}[1m\u{1b}[31mfailed\u{1b}[39m ok", "\u{1b}[22mplain"]));
        let (LogEntry::Line(a), LogEntry::Line(b)) = (&out[0], &out[1]) else { panic!("two lines") };
        let bold = Style { bold: true, ..Style::default() };
        assert_eq!(a.spans, [Span { text: "failed".into(), style: Style { fg: Some(1), ..bold } }, Span { text: " ok".into(), style: bold }]);
        assert_eq!(b.spans, [Span { text: "plain".into(), style: Style::default() }]);
    }

    #[test]
    fn parse_log_folds_gitlabs_sections_which_can_end_and_start_on_one_line_and_keeps_what_a_progress_redraw_left() {
        let out = parse_log(&lines(&[
            "section_start:1:prepare\r\u{1b}[0K\u{1b}[36;1mPreparing\u{1b}[0;m",
            "Using docker",
            "section_end:1:prepare\r\u{1b}[0Ksection_start:2:script[collapsed=true]\r\u{1b}[0KRunning script",
            "10%\r50%\r100%",
            "section_end:2:script\r\u{1b}[0K",
            "Job succeeded",
        ]));
        let shown: Vec<String> = out
            .iter()
            .map(|e| match e {
                LogEntry::Group { title, lines } => format!("{}: {}", text(title), lines.iter().map(text).collect::<Vec<_>>().join(",")),
                LogEntry::Line(l) => text(l),
            })
            .collect();
        assert_eq!(shown, ["Preparing: Using docker", "Running script: 100%", "Job succeeded"]);
    }

    #[test]
    fn step_lines_cuts_an_actions_jobs_log_into_steps_that_start_in_the_same_second() {
        let at = |s: u8, rest: &str| format!("2026-10-17T06:55:0{s}.1000000Z {rest}");
        let raw = [at(8, "Current runner version"), at(9, "##[group] Inputs"), at(9, "##[endgroup]"), at(9, "##[group]Run actions/checkout@v4"), at(9, "with:"), at(9, "Cleaning up orphan processes")];
        let step = |a: &str, b: &str| (Some(a.to_owned()), Some(b.to_owned()));
        let parts = step_lines(&raw, &[step("2026-10-17T06:55:08Z", "2026-10-17T06:55:09Z"), step("2026-10-17T06:55:09Z", "2026-10-17T06:55:09Z"), step("2026-10-17T06:55:09Z", "2026-10-17T06:55:10Z")]);
        assert_eq!(parts.iter().map(Vec::len).collect::<Vec<_>>(), [3, 2, 1]);
    }
}
