//! Turning a failed check's log into the line that says why and the end of the log.

use crate::runlog::without_markup;
use crate::types::FailureDetail;

const LOG_LINES: usize = 150;
const LOG_CHARS: usize = 8_000;
pub const TOOL_CHARS: usize = 30_000;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A match of `needle` in `text` that doesn't continue a word before it, then what follows it.
fn after_word<'a>(text: &'a str, needle: &str) -> Vec<&'a str> {
    text.match_indices(needle).filter(|(at, _)| !text[..*at].chars().next_back().is_some_and(is_word)).map(|(at, _)| &text[at + needle.len()..]).collect()
}

/// These lines say a step failed, not why.
fn generic(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("error: job failed")
        || ["exit", "exited", "exiting", "completed", "failed"].iter().any(|word| after_word(&lower, &format!("{word} with ")).into_iter().any(|rest| rest.starts_with("code") || rest.starts_with("exit code")))
}

/// A line the tool marked as an error beats one that only mentions errors, like a table header.
fn marked(line: &str) -> bool {
    let lower = line.to_lowercase();
    let start = lower.trim_start();
    let alone = |at: usize, len: usize| !lower[..at].chars().next_back().is_some_and(is_word) && !lower[at + len..].chars().next().is_some_and(is_word);
    ["[error]", "[fail]", "[failed]", "[fatal]"].iter().any(|m| lower.contains(m))
        || ["error", "fatal", "failed"].iter().any(|word| start.strip_prefix(word).is_some_and(|rest| !rest.chars().next().is_some_and(is_word)))
        || ["level=error", "level=fatal"].iter().any(|m| lower.match_indices(m).any(|(at, _)| alone(at, m.len())))
        || start.starts_with(['✖', '✗', '×'])
}

fn errorish(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["error", "fail", "violation", "vulnerab", "[warn]", "panic", "exception"].iter().any(|m| lower.contains(m))
}

/// `text` without `ESC [ params letter` sequences.
fn without_escapes(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\u{1b}[") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let params = tail.find(|c: char| !(c.is_ascii_digit() || c == ';' || c == '?')).unwrap_or(tail.len());
        match tail[params..].chars().next().filter(char::is_ascii_alphabetic) {
            Some(letter) => rest = &tail[params + letter.len_utf8()..],
            None => {
                out.push_str("\u{1b}[");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `text` without GitLab's `section_start:1234:name` and `section_end:...` markers, each up to its line's end and a `\r`.
fn without_sections(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("section_") {
        let tail = &rest[at + "section_".len()..];
        let body = tail.strip_prefix("start:").or_else(|| tail.strip_prefix("end:"));
        let digits = body.map(|b| b.bytes().take_while(u8::is_ascii_digit).count());
        match (body, digits) {
            (Some(body), Some(n)) if n > 0 && body[n..].starts_with(':') => {
                out.push_str(&rest[..at]);
                let after = &body[n + 1..];
                let end = after.find(['\r', '\n']).unwrap_or(after.len());
                rest = after[end..].strip_prefix('\r').unwrap_or(&after[end..]);
            }
            _ => {
                out.push_str(&rest[..at + "section_".len()]);
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The length of a leading `2026-10-16T17:03:38.45Z ` stamp, space included.
fn timestamp_len(line: &str) -> usize {
    let b = line.as_bytes();
    let digits = |from: usize, n: usize| b.get(from..from + n).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    if !(digits(0, 4) && b.get(4) == Some(&b'-') && digits(5, 2) && b.get(7) == Some(&b'-') && digits(8, 2) && b.get(10) == Some(&b'T')) {
        return 0;
    }
    let run = b[11..].iter().take_while(|c| c.is_ascii_digit() || **c == b':' || **c == b'.').count();
    if run == 0 || b.get(11 + run) != Some(&b'Z') {
        return 0;
    }
    let end = 11 + run + 1;
    if b.get(end) == Some(&b' ') { end + 1 } else { end }
}

/// Log lines without colours, timestamps, GitLab section markers, progress redraws or the CI's `##[...]` markup.
pub fn clean_log(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let text = without_sections(&without_escapes(line));
            let Some(last) = text.split('\r').rfind(|part| !part.is_empty()) else {
                return String::new();
            };
            let last = &last[timestamp_len(last)..];
            let rest = match last.strip_prefix("##[error]") {
                Some(rest) => format!("error: {rest}"),
                None => match last.strip_prefix("##[warning]") {
                    Some(rest) => format!("warning: {rest}"),
                    None => without_markup(last).to_owned(),
                },
            };
            rest.trim_end().to_owned()
        })
        .collect()
}

/// The line that best says what went wrong: the CI's own error, else the last line that reads like one.
fn summarize(lines: &[String], issues: &[String]) -> String {
    let text: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty() && !generic(l.trim())).collect();
    issues
        .iter()
        .find(|m| !generic(m))
        .or_else(|| text.iter().rfind(|l| marked(l)).copied())
        .or_else(|| text.iter().rfind(|l| errorish(l)).copied())
        .or_else(|| text.last().copied())
        .map_or("Failed", |line| line.as_str())
        .trim()
        .to_owned()
}

/// The end of the log, where the error is.
fn tail(lines: &[String]) -> String {
    let start = lines.len().saturating_sub(LOG_LINES);
    let text = lines[start..].join("\n");
    let text = text.trim();
    let chars = text.chars().count();
    if chars <= LOG_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().skip(chars - LOG_CHARS).collect();
    match cut.split_once('\n') {
        Some((_, rest)) => rest.to_owned(),
        None => cut,
    }
}

pub fn detail(check: Option<&str>, step: Option<&str>, lines: &[String], issues: &[String], url: Option<&str>) -> FailureDetail {
    FailureDetail { check: check.map(str::to_owned), step: step.map(str::to_owned), summary: summarize(lines, issues), log: tail(lines), url: url.map(str::to_owned) }
}

/// A fence of backticks longer than any run of them in the text (and at least 3), so the text can't close it and continue as plain text.
pub fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat(3.max(if longest > 0 { longest + 1 } else { 0 }))
}

pub fn fenced(text: &str) -> String {
    let bar = fence(text);
    format!("{bar}text\n{text}\n{bar}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_log_loses_colours_timestamps_section_markers_progress_redraws_and_the_cis_markup() {
        let out = clean_log(&lines(&[
            "2026-10-16T17:03:38.4518137Z ##[error]Process completed with exit code 1.  ",
            "\u{1b}[31mred\u{1b}[0m text",
            "10%\r50%\r100%",
            "section_start:1:script\r\u{1b}[0KRunning",
            "2026-10-16T17:03:38Z ##[group]Run build",
            "",
        ]));
        assert_eq!(out, ["error: Process completed with exit code 1.", "red text", "100%", "Running", "Run build", ""]);
    }

    #[test]
    fn the_summary_is_the_cis_own_error_else_the_last_line_that_reads_like_one_and_never_a_line_that_only_says_it_failed() {
        let log = lines(&["compiling", "error[E0308]: mismatched types", "see above", "Process completed with exit code 1."]);
        assert_eq!(detail(Some("build"), None, &log, &[], None).summary, "error[E0308]: mismatched types");
        let issues = lines(&["Bash exited with code '1'.", "Missing license header"]);
        assert_eq!(detail(None, None, &log, &issues, None).summary, "Missing license header");
        assert_eq!(detail(None, None, &lines(&["one", "two"]), &[], None).summary, "two");
        assert_eq!(detail(None, None, &[], &[], None).summary, "Failed");
        assert_eq!(detail(None, None, &lines(&["Table of errors", "[ERROR] boom", "done"]), &[], None).summary, "[ERROR] boom");
        assert_eq!(detail(None, None, &lines(&["ERROR: Job failed: exit code 1"]), &[], None).summary, "Failed");
    }

    #[test]
    fn the_log_keeps_its_end_and_cuts_a_long_one_at_a_line_start() {
        let long: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
        assert!(detail(None, None, &long, &[], None).log.starts_with("line 150"));
        let wide = vec!["x".repeat(5000), "y".repeat(5000)];
        let log = detail(None, None, &wide, &[], None).log;
        assert_eq!(log, "y".repeat(5000));
    }

    #[test]
    fn a_fence_is_longer_than_any_backtick_run_in_the_text() {
        assert_eq!(fenced("a"), "```text\na\n```");
        assert_eq!(fenced("a ```` b"), "`````text\na ```` b\n`````");
    }
}
