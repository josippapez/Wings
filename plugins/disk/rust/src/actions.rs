//! What the tab asks main to do on the disk: move the Collector to the Trash, show a file, and manage the local snapshots.

use crate::{
    collector,
    disks::{disks_now, is_snapshot_date},
    gate::Gate,
    protect::protected_reason,
    scans::{recompute_hidden, remove_from_scans},
    state::{Collected, TrashResult},
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};
use wings_plugin::{host, task::map_limit};

/// What a command printed, trimmed: its errors, else its output.
fn said(out: &Value) -> String {
    let text = |name: &str| out[name].as_str().unwrap_or_default().trim().to_owned();
    let errors = text("stderr");
    if errors.is_empty() { text("stdout") } else { errors }
}

/// An error message for a command that didn't succeed.
fn failure(program: &str, out: &Value) -> String {
    let said = said(out);
    if !said.is_empty() {
        return said;
    }
    match out["code"].as_i64() {
        Some(code) => format!("{program} failed with code {code}."),
        None => format!("{program} failed."),
    }
}

/// Anything that is handed to a program as a path must be one, and so can't be taken for a flag.
fn assert_full_path(path: &str) -> Result<(), String> {
    if path.starts_with('/') { Ok(()) } else { Err(format!("{path} isn't a full path.")) }
}

/// Why `trash` failed, as it says it: it wraps the reason in a time stamp, its process id and a dump of the error
/// (`... trash[123:456] # Error attempting to move /x to the trash folder, Error Domain=NSCocoaErrorDomain Code=4 "The file
/// “x” doesn’t exist." UserInfo={...}`).
fn trash_reason(out: &Value) -> String {
    let text = said(out);
    if let Some(reason) = system_reason(&text) {
        return reason.to_owned();
    }
    let rest = strip_stamp(&text);
    if rest.is_empty() { failure("trash", out) } else { rest.to_owned() }
}

/// The `The file “x” doesn’t exist.` in `Error Domain=NSCocoaErrorDomain Code=4 "The file “x” doesn’t exist."`.
fn system_reason(text: &str) -> Option<&str> {
    let after_label = &text[text.find("Error Domain=")? + "Error Domain=".len()..];
    let after_domain = &after_label[after_label.find(' ').filter(|at| *at > 0)?..];
    let code = after_domain.strip_prefix(" Code=")?;
    let digits = code.strip_prefix('-').unwrap_or(code);
    let count = digits.bytes().take_while(u8::is_ascii_digit).count();
    let quoted = digits[count..].strip_prefix(" \"").filter(|_| count > 0)?;
    quoted.split_once('"').map(|(reason, _)| reason).filter(|reason| !reason.is_empty())
}

/// `text` without the `2026-11-12 10:00:00.123 trash[123:456] ` that begins a line `trash` writes.
fn strip_stamp(text: &str) -> &str {
    let bytes = text.as_bytes();
    let date = bytes.len() > 11 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-' && bytes[7] == b'-' && bytes[10] == b' ';
    if !date {
        return text;
    }
    let after = &text[11..];
    let time = after.bytes().take_while(|byte| byte.is_ascii_digit() || matches!(byte, b':' | b'.')).count();
    match after[time..].strip_prefix(" trash[").and_then(|rest| rest.split_once("] ")) {
        Some((ids, rest)) if ids.bytes().all(|byte| byte.is_ascii_digit() || byte == b':') => rest,
        _ => text,
    }
}

struct Trashing {
    done: Gate,
    result: RefCell<Option<TrashResult>>,
}

thread_local! {
    static TRASHING: RefCell<Option<Rc<Trashing>>> = const { RefCell::new(None) };
}

/// Moves everything in the Collector to the Trash, and gives what moved and what didn't. A second call while one runs gets that one's result.
pub async fn trash_collected() -> Result<TrashResult, String> {
    if let Some(running) = TRASHING.with(|trashing| trashing.borrow().clone()) {
        running.done.wait().await;
        return running.result.borrow().clone().ok_or_else(|| "Moving to the Trash didn't finish.".to_owned());
    }
    let running = Rc::new(Trashing { done: Gate::default(), result: RefCell::new(None) });
    TRASHING.with(|trashing| *trashing.borrow_mut() = Some(running.clone()));
    let moved = move_to_trash().await;
    *running.result.borrow_mut() = moved.as_ref().ok().cloned();
    TRASHING.with(|trashing| *trashing.borrow_mut() = None);
    running.done.open();
    moved
}

async fn move_to_trash() -> Result<TrashResult, String> {
    let items = collector::now().items;
    let mounts: Vec<String> = disks_now(30_000.0).await?.rows.into_iter().map(|d| d.disk.mount).collect();
    // Each item is checked again where it matters most, so one stored by an older version can't get past what is refused now.
    let errors: Vec<Option<String>> = map_limit(items.clone(), 4, |item| {
        let mounts = mounts.clone();
        async move {
            if let Some(reason) = protected_reason(&item.path, &mounts) {
                return Some(reason);
            }
            match host::exec("trash", &[item.path.as_str()]).await {
                Ok(out) if out["code"] == 0 => None,
                Ok(out) => Some(trash_reason(&out)),
                Err(error) => Some(error),
            }
        }
    })
    .await;
    let (mut trashed, mut failed): (Vec<Collected>, Vec<(String, String)>) = (Vec::new(), Vec::new());
    for (item, error) in items.into_iter().zip(errors) {
        match error {
            None => trashed.push(item),
            Some(message) => failed.push((item.path, message)),
        }
    }
    remove_from_scans(&trashed);
    let paths: Vec<String> = trashed.iter().map(|item| item.path.clone()).collect();
    collector::uncollect(&paths)?;
    wings_plugin::messages::tell(crate::state::REFRESH, &Value::Null);
    Ok(TrashResult { freed: trashed.iter().map(|item| item.size).sum(), trashed: paths, failed })
}

/// Shows `path` in Finder.
pub async fn reveal(path: &str) -> Result<(), String> {
    assert_full_path(path)?;
    let out = host::exec("open", &["-R", path]).await?;
    if out["code"] == 0 { Ok(()) } else { Err(failure("open", &out)) }
}

/// Opens Quick Look on `path` and returns: it stays open until the user closes it.
pub fn preview(path: &str) -> Result<(), String> {
    assert_full_path(path)?;
    // exec ends Quick Look after 5 minutes at most, which rejects it: that is expected, and nothing to report.
    drop(host::exec_timeout("qlmanage", &["-p", path], 300_000));
    Ok(())
}

/// The disks, and the hidden space of the scanned disks, once space was freed.
async fn space_freed() {
    wings_plugin::messages::tell(crate::state::REFRESH, &Value::Null);
    recompute_hidden().await;
}

pub async fn delete_snapshot(date: &str) -> Result<(), String> {
    if !is_snapshot_date(date) {
        return Err(format!("{date} isn't the date of a local snapshot."));
    }
    let out = host::exec("tmutil", &["deletelocalsnapshots", date]).await?;
    if out["code"] != 0 {
        return Err(failure("tmutil", &out));
    }
    space_freed().await;
    Ok(())
}

/// Asks macOS to free the purgeable space of the disk at `mount` by thinning its local snapshots. It can take minutes.
pub async fn purge(mount: &str) -> Result<(), String> {
    if !disks_now(30_000.0).await?.rows.iter().any(|d| d.disk.mount == mount) {
        return Err(format!("{mount} isn't a disk."));
    }
    let out = host::exec_timeout("tmutil", &["thinlocalsnapshots", mount, "999999999999", "4"], 300_000).await?;
    if out["code"] != 0 {
        return Err(failure("tmutil", &out));
    }
    space_freed().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn output(stdout: &str, stderr: &str, code: i64) -> Value {
        json!({ "code": code, "stdout": stdout, "stderr": stderr })
    }

    #[test]
    fn trash_failures_are_told_by_the_reason_the_system_gave() {
        let wrapped = "2026-11-12 10:00:00.123 trash[123:456] # Error attempting to move /x to the trash folder, Error Domain=NSCocoaErrorDomain Code=4 \"The file “x” doesn’t exist.\" UserInfo={NSUserStringVariant=(Move)}";
        assert_eq!(trash_reason(&output("", wrapped, 1)), "The file “x” doesn’t exist.");
        assert_eq!(trash_reason(&output("", "2026-11-12 10:00:00.123 trash[123:456] Operation not permitted", 1)), "Operation not permitted");
        assert_eq!(trash_reason(&output("", "plain words", 1)), "plain words");
        assert_eq!(trash_reason(&output("", "", 64)), "trash failed with code 64.");
        assert_eq!(failure("open", &json!({ "code": null, "stdout": "", "stderr": "" })), "open failed.");
        assert_eq!(said(&output("out", "  ", 0)), "out");
    }

    #[test]
    fn a_program_is_only_handed_a_full_path() {
        assert!(assert_full_path("/Users/ada/x").is_ok());
        assert_eq!(assert_full_path("-rf").unwrap_err(), "-rf isn't a full path.");
        assert_eq!(wings_plugin::task::block_on(delete_snapshot("-d")).unwrap_err(), "-d isn't the date of a local snapshot.");
    }
}
