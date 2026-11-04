//! Codex's OSC 7501 lifecycle reports, independent of title preferences.
//! Source: https://github.com/openai/codex/blob/rust-v0.166.0/codex-rs/tui/src/terminal_program_status.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexStatus {
    Idle,
    Working,
    Blocked,
    Permission,
    Question,
    Auth,
    Done,
    Error,
}

/// One actual report, rather than a detector rereading the last report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexReport {
    pub state: CodexStatus,
    pub generation: u64,
    /// The foreground process when the report arrived, where the PTY exposes it.
    pub pid: Option<u32>,
}

/// None ignores a report; Some(None) clears the previous report.
pub fn codex_report(report: &[u8]) -> Option<Option<CodexStatus>> {
    let (mut state, mut app, mut kind) = (None, None, None);
    for field in report.split(|&b| b == b':') {
        let at = field.iter().position(|&b| b == b'=')?;
        let (key, value) = (&field[..at], &field[at + 1..]);
        match key {
            b"state" if state.is_none() => state = Some(value),
            b"app" if app.is_none() => app = Some(value),
            b"kind" if kind.is_none() => kind = Some(value),
            // Root state only: a child agent must not overwrite its parent's status.
            b"id" | b"parent" | b"state" | b"app" | b"kind" => return None,
            _ => {}
        }
    }
    if state == Some(b"clear") && (app.is_none() || app == Some(b"codex")) {
        return Some(None);
    }
    if app != Some(b"codex") {
        return None;
    }
    Some(Some(match state? {
        b"idle" => CodexStatus::Idle,
        b"working" => CodexStatus::Working,
        b"blocked" => match kind {
            Some(b"permission") => CodexStatus::Permission,
            Some(b"question") => CodexStatus::Question,
            Some(b"auth") => CodexStatus::Auth,
            _ => CodexStatus::Blocked,
        },
        b"done" => CodexStatus::Done,
        b"error" => CodexStatus::Error,
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_codex_lifecycle_and_blocking_reasons() {
        for (report, want) in [
            ("state=idle:app=codex", CodexStatus::Idle),
            ("state=working:app=codex", CodexStatus::Working),
            ("state=blocked:app=codex", CodexStatus::Blocked),
            ("state=blocked:app=codex:kind=permission", CodexStatus::Permission),
            ("app=codex:kind=question:state=blocked", CodexStatus::Question),
            ("state=blocked:kind=auth:app=codex", CodexStatus::Auth),
            ("state=done:app=codex", CodexStatus::Done),
            ("state=error:app=codex", CodexStatus::Error),
        ] {
            assert_eq!(codex_report(report.as_bytes()), Some(Some(want)), "{report}");
        }
        assert_eq!(codex_report(b"state=clear"), Some(None));
        assert_eq!(codex_report(b"state=clear:app=codex"), Some(None));
    }

    #[test]
    fn ignores_probes_other_apps_subagents_and_malformed_reports() {
        for report in ["?", "state=working", "state=working:app=claude", "state=clear:app=other", "state=done:app=codex:id=child", "state=working:app=codex:parent=root", "state=unknown:app=codex", "state=working:state=done:app=codex", "state=working:app=codex:app=other"] {
            assert_eq!(codex_report(report.as_bytes()), None, "{report}");
        }
    }
}
