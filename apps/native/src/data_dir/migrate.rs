//! The replacement's first start on the desktop's folder: what the side-by-side app kept apart comes over, and the
//! desktop's tabs and settings become the native app's.
//!
//! - `native-workspace.json` is made once, from the side-by-side `workspace.json` and the desktop's `workspace.json`.
//!   The desktop's tabs and settings win where both have one, since the desktop is the app you were using; projects and
//!   settings only the side-by-side app had are kept.
//! - `spaces.json` gets the projects only the side-by-side list had, appended. The desktop's file is copied into
//!   `backups/` first.
//! - `scrollback/` gets the side-by-side panes' saved output under names the desktop's folder doesn't use yet.
//!
//! The side-by-side folder is only read, and the desktop's `workspace.json`, plugins, plugin storage and project pages
//! are never written, so nothing either app kept is lost. `native-migration.json` records that it ran: later starts
//! leave both folders alone, so a project you remove afterwards stays removed.

use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use super::{REPLACEMENT_STATE, desktop};
use crate::saved::{self, Settings, valid_history_id};

/// Written once the migration is done.
pub const MARKER: &str = "native-migration.json";

/// What one run did.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// An earlier start already migrated, so nothing was done.
    #[serde(skip)]
    pub already: bool,
    /// Projects whose tabs came from the desktop's workspace.
    pub desktop_projects: usize,
    /// Projects whose tabs came from the side-by-side workspace.
    pub native_projects: usize,
    /// Projects added to the desktop's `spaces.json`.
    pub added_spaces: usize,
    /// Saved pane outputs copied into `scrollback/`.
    pub copied_scrollback: usize,
    /// Saved outputs with a name the desktop's folder already had for other output; the desktop's were kept.
    pub scrollback_conflicts: usize,
    /// Where the files this replaced were copied first.
    pub backup: Option<PathBuf>,
}

/// Moves the side-by-side app's data in `from` into the desktop's folder `to`. `now` (ms since the epoch) names the
/// backup folder.
pub fn migrate(from: &Path, to: &Path, now: u64) -> io::Result<Report> {
    let marker = to.join(MARKER);
    if marker.exists() {
        return Ok(Report {
            already: true,
            ..Report::default()
        });
    }
    std::fs::create_dir_all(to)?;
    let backup = to.join("backups").join(format!("native-switch-{now}"));
    let mut report = Report::default();
    workspace(from, to, &mut report)?;
    spaces(from, to, &backup, &mut report)?;
    scrollback(from, to, &mut report)?;
    let record = serde_json::json!({
        "version": 1,
        "from": from,
        "at": now,
        "report": report,
    });
    write(&marker, &serde_json::to_vec_pretty(&record)?)?;
    Ok(report)
}

/// Makes `native-workspace.json`, unless an earlier, unfinished run already did.
fn workspace(from: &Path, to: &Path, report: &mut Report) -> io::Result<()> {
    let state = to.join(REPLACEMENT_STATE);
    if state.exists() {
        return Ok(());
    }
    let (native, mut projects) = saved::read_all(&from.join("workspace.json")).unwrap_or_default();
    let desktop = desktop::read(&to.join("workspace.json"));
    let settings = match &desktop {
        Some(desktop) => overlay(native, desktop.settings())?,
        None => native,
    };
    let desktop = desktop
        .map(|desktop| desktop.projects())
        .unwrap_or_default();
    report.desktop_projects = desktop.len();
    report.native_projects = projects
        .keys()
        .filter(|key| !desktop.contains_key(*key))
        .count();
    projects.extend(desktop);
    saved::write_all(&state, settings, projects)
}

/// `top`'s settings over `base`'s. Each project's last-worked-on time is the later of the two.
fn overlay(base: Settings, top: Settings) -> io::Result<Settings> {
    let (Value::Object(mut base), Value::Object(top)) =
        (serde_json::to_value(base)?, serde_json::to_value(top)?)
    else {
        unreachable!("settings serialize as an object");
    };
    for (key, value) in top {
        match (base.get_mut(&key), value) {
            (Some(Value::Object(times)), Value::Object(newer)) if key == "last_active" => {
                for (project, time) in newer {
                    let later = times
                        .get(&project)
                        .and_then(Value::as_u64)
                        .is_none_or(|old| time.as_u64().is_some_and(|new| new > old));
                    if later {
                        times.insert(project, time);
                    }
                }
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
    Ok(serde_json::from_value(Value::Object(base))?)
}

/// Appends the projects only the side-by-side list has, keeping every field of each entry as it is.
fn spaces(from: &Path, to: &Path, backup: &Path, report: &mut Report) -> io::Result<()> {
    let list = |path: &Path| -> Vec<Value> {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    };
    let target = to.join("spaces.json");
    let mut spaces = list(&target);
    let id = |space: &Value| space.get("id").and_then(Value::as_str).map(str::to_owned);
    let known: HashSet<String> = spaces.iter().filter_map(id).collect();
    let added: Vec<Value> = list(&from.join("spaces.json"))
        .into_iter()
        .filter(|space| id(space).is_some_and(|id| !known.contains(&id)))
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    if target.exists() {
        std::fs::create_dir_all(backup)?;
        std::fs::copy(&target, backup.join("spaces.json"))?;
        report.backup = Some(backup.to_owned());
    }
    report.added_spaces = added.len();
    spaces.extend(added);
    write(&target, serde_json::to_string_pretty(&spaces)?.as_bytes())
}

/// Copies the side-by-side panes' saved output in. A name already taken keeps the desktop's file.
fn scrollback(from: &Path, to: &Path, report: &mut Report) -> io::Result<()> {
    let Ok(entries) = std::fs::read_dir(from.join("scrollback")) else {
        return Ok(());
    };
    let dir = to.join("scrollback");
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if !entry.file_type()?.is_file() || !name.to_str().is_some_and(valid_history_id) {
            continue;
        }
        let target = dir.join(&name);
        if target.exists() {
            if std::fs::read(&target)? != std::fs::read(entry.path())? {
                report.scrollback_conflicts += 1;
            }
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(entry.path(), target)?;
        report.copied_scrollback += 1;
    }
    Ok(())
}

/// Write then rename, so a crash can't leave half a file.
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}
