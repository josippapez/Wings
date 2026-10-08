//! Finds Claude Code running inside Wings panes and works out what it is doing.
//! No Claude config changes: the process tree tells us a pane runs `claude`, Claude's own
//! `sessions/<pid>.json` gives the session id and status, and the OSC title is the fallback.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

mod launch_args;
use launch_args::launch_args;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    Working,
    Blocked,
    /// Finished a turn while you were looking elsewhere.
    Done,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub pane_id: String,
    pub space_id: String,
    pub pid: u32,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub state: AgentState,
    pub waiting_for: Option<String>,
    /// The flags `claude` was started with, to type again when Wings resumes the session.
    pub args: Vec<String>,
}

/// Claude Code's live registry entry, `~/.claude/sessions/<pid>.json`. Undocumented, so every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
    pub name: Option<String>,
}

pub struct Scan {
    pub agents: Vec<Agent>,
    /// What each pane is running right now, by pane id.
    pub panes: HashMap<String, PaneInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneInfo {
    /// `zsh`, `claude`, `bun`…
    pub command: String,
    /// Where that process is now, so `cd` in the shell moves the pane with it.
    pub cwd: Option<String>,
}

pub struct PaneProbe {
    pub pane_id: String,
    pub space_id: String,
    pub shell_pid: Option<u32>,
    /// The PTY's foreground process group leader (`tcgetpgrp`). Unix only; `None` on Windows.
    pub foreground_pid: Option<u32>,
    pub title: String,
}

pub struct Detector {
    system: System,
    sessions_dir: PathBuf,
    last: HashMap<String, AgentState>,
    unseen_done: HashMap<String, bool>,
    /// Dev builds log each state change, to debug detection against a live Claude.
    traced: HashMap<String, (AgentState, AgentState, bool)>,
}

impl Detector {
    pub fn new(claude_dir: &Path) -> Self {
        Self {
            system: System::new(),
            sessions_dir: claude_dir.join("sessions"),
            last: HashMap::new(),
            unseen_done: HashMap::new(),
            traced: HashMap::new(),
        }
    }

    pub fn scan(&mut self, panes: &[PaneProbe], focused: Option<&str>) -> Scan {
        // With a foreground pid per pane only those processes need reading; a full scan costs ~7 ms.
        let foreground: Vec<Pid> = panes.iter().filter_map(|p| p.foreground_pid).map(Pid::from_u32).collect();
        let full_scan = foreground.len() < panes.len();
        let kind = ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet);
        let targets = if full_scan { ProcessesToUpdate::All } else { ProcessesToUpdate::Some(&foreground) };
        self.system.refresh_processes_specifics(targets, true, kind);
        let children = if full_scan { children_by_parent(self.system.processes()) } else { HashMap::new() };
        let jobs: Vec<Pid> = if full_scan {
            panes.iter().filter_map(|p| p.shell_pid).flat_map(|shell| descendants(&children, Pid::from_u32(shell))).collect()
        } else {
            foreground.clone()
        };
        reread_argv(&mut self.system, &jobs);
        let processes = self.system.processes();

        // Per pane: the process it is running now, and the Claude process if that is one.
        let found: Vec<(Option<Pid>, Option<Pid>)> = panes
            .iter()
            .map(|pane| match (pane.foreground_pid.map(Pid::from_u32), pane.shell_pid.map(Pid::from_u32)) {
                (Some(fg), _) => (Some(fg), processes.get(&fg).filter(|p| is_claude(p)).map(|_| fg)),
                (None, Some(shell)) => (Some(newest_child(&children, shell)), find_claude(processes, &children, shell)),
                (None, None) => (None, None),
            })
            .collect();
        // Read from the argv the scan above loaded to recognise `claude`.
        let launch: Vec<Vec<String>> = found
            .iter()
            .map(|(_, claude)| claude.and_then(|pid| processes.get(&pid)).map(|p| launch_args(p.cmd())).unwrap_or_default())
            .collect();
        // The working directory changes with every `cd`, so it is re-read each scan, for these processes only.
        let current: Vec<Pid> = found.iter().filter_map(|(pid, _)| *pid).collect();
        self.system.refresh_processes_specifics(ProcessesToUpdate::Some(&current), false, ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always));
        let processes = self.system.processes();
        let infos: Vec<PaneInfo> = found
            .iter()
            .map(|(pid, _)| PaneInfo {
                command: pid.map(|p| name_of(processes, p)).unwrap_or_default(),
                cwd: pid.and_then(|p| processes.get(&p)?.cwd()).map(|c| c.to_string_lossy().into_owned()),
            })
            .collect();

        let mut agents = Vec::new();
        let mut pane_infos = HashMap::new();
        for (((pane, (_, claude)), info), args) in panes.iter().zip(found).zip(infos).zip(launch) {
            pane_infos.insert(pane.pane_id.clone(), info);
            let Some(pid) = claude else {
                self.last.remove(&pane.pane_id);
                self.unseen_done.remove(&pane.pane_id);
                continue;
            };
            let file = read_session_file(&self.sessions_dir, pid.as_u32()).unwrap_or_default();
            let raw = file
                .status
                .as_deref()
                .and_then(state_from_status)
                .or_else(|| state_from_title(&pane.title))
                .unwrap_or(AgentState::Idle);
            let is_focused = focused == Some(pane.pane_id.as_str());
            let state = self.with_done(&pane.pane_id, raw, is_focused);
            if cfg!(debug_assertions) && self.traced.insert(pane.pane_id.clone(), (raw, state, is_focused)) != Some((raw, state, is_focused)) {
                eprintln!("[detect] {} raw={raw:?} shown={state:?} focused={is_focused} status={:?}", pane.pane_id, file.status);
            }
            agents.push(Agent {
                pane_id: pane.pane_id.clone(),
                space_id: pane.space_id.clone(),
                pid: pid.as_u32(),
                session_id: file.session_id,
                name: file.name.or_else(|| strip_title_glyph(&pane.title)),
                state,
                waiting_for: if state == AgentState::Blocked { file.waiting_for } else { None },
                args,
            });
        }
        Scan { agents, panes: pane_infos }
    }

    /// Turns working → idle into "done" until the pane gets focus.
    fn with_done(&mut self, pane_id: &str, raw: AgentState, focused: bool) -> AgentState {
        let prev = self.last.insert(pane_id.to_string(), raw);
        let unseen = self.unseen_done.entry(pane_id.to_string()).or_insert(false);
        if raw != AgentState::Idle || focused {
            *unseen = false;
        } else if prev == Some(AgentState::Working) {
            *unseen = true;
        }
        if *unseen { AgentState::Done } else { raw }
    }
}

/// A process keeps its pid when it execs another program, and sysinfo keeps the argv and name it cached. One
/// caught between a shell's fork and its exec of `claude` would read as the shell for good, so the processes
/// that can be a pane's job get their argv read again on every scan.
fn reread_argv(system: &mut System, pids: &[Pid]) {
    system.refresh_processes_specifics(ProcessesToUpdate::Some(pids), false, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
}

fn descendants(children: &HashMap<Pid, Vec<Pid>>, root: Pid) -> Vec<Pid> {
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        for child in children.get(&pid).into_iter().flatten() {
            found.push(*child);
            queue.push_back(*child);
        }
    }
    found
}

fn children_by_parent(processes: &HashMap<Pid, Process>) -> HashMap<Pid, Vec<Pid>> {
    let mut map: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, process) in processes {
        if let Some(parent) = process.parent() {
            map.entry(parent).or_default().push(*pid);
        }
    }
    map
}

/// Breadth-first, so the shallowest `claude` wins over one that Claude itself started.
fn find_claude(
    processes: &HashMap<Pid, Process>,
    children: &HashMap<Pid, Vec<Pid>>,
    root: Pid,
) -> Option<Pid> {
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        if pid != root && processes.get(&pid).is_some_and(is_claude) {
            return Some(pid);
        }
        queue.extend(children.get(&pid).into_iter().flatten().copied());
    }
    None
}

/// Windows fallback, where there is no foreground process group: the shell's newest child, or the shell itself.
// debt: newest child approximates the foreground job; background jobs can fool it.
fn newest_child(children: &HashMap<Pid, Vec<Pid>>, shell: Pid) -> Pid {
    children.get(&shell).and_then(|kids| kids.iter().max().copied()).unwrap_or(shell)
}

fn name_of(processes: &HashMap<Pid, Process>, pid: Pid) -> String {
    let Some(process) = processes.get(&pid) else { return String::new() };
    // From argv, which is re-read each scan; the cached name is still the old program's after an exec.
    let name = match process.cmd().first() {
        Some(argv0) => Path::new(argv0).file_name().unwrap_or(argv0).to_string_lossy().into_owned(),
        None => process.name().to_string_lossy().into_owned(),
    };
    // Login shells are named `-zsh`.
    name.trim_start_matches('-').to_string()
}

fn is_claude(process: &Process) -> bool {
    let argv0 = process.cmd().first().map(|a| Path::new(a).file_name().unwrap_or(a));
    process.name() == "claude"
        || argv0 == Some(OsStr::new("claude"))
        // npm installs run as `node …/@anthropic-ai/claude-code/cli.js`
        || process.cmd().iter().any(|a| a.to_string_lossy().contains("@anthropic-ai/claude-code"))
}

fn read_session_file(dir: &Path, pid: u32) -> Option<SessionFile> {
    let text = std::fs::read_to_string(dir.join(format!("{pid}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

/// `status` values documented for `claude agents --json`: busy, waiting, idle.
pub fn state_from_status(status: &str) -> Option<AgentState> {
    match status {
        "busy" => Some(AgentState::Working),
        "waiting" => Some(AgentState::Blocked),
        "idle" => Some(AgentState::Idle),
        _ => None,
    }
}

/// Claude Code titles start with a braille or half-circle spinner while working and `✳` when idle.
pub fn state_from_title(title: &str) -> Option<AgentState> {
    match title.chars().next()? {
        '\u{2800}'..='\u{28FF}' | '\u{25D0}'..='\u{25D3}' => Some(AgentState::Working),
        '✳' => Some(AgentState::Idle),
        _ => None,
    }
}

fn strip_title_glyph(title: &str) -> Option<String> {
    state_from_title(title)?;
    let rest = title.chars().skip(1).collect::<String>().trim().to_string();
    (!rest.is_empty()).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command, time::Duration};

    #[test]
    fn sees_a_process_that_becomes_another_program() {
        // `sh` waits, then execs `sleep`: the same pid, a new program.
        let mut child = Command::new("/bin/sh").args(["-c", "sleep 0.4; exec /bin/sleep 5"]).spawn().unwrap();
        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        let cached = ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet);
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, cached);
        let before = name_of(system.processes(), pid);
        std::thread::sleep(Duration::from_millis(900));
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, cached);
        let stale = name_of(system.processes(), pid);
        reread_argv(&mut system, &[pid]);
        let after = name_of(system.processes(), pid);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(before, "sh");
        assert_eq!(stale, "sh", "sysinfo keeps the argv it read before the exec");
        assert_eq!(after, "sleep");
    }

    #[test]
    fn maps_title_glyphs() {
        assert_eq!(state_from_title("⠂ Refactor auth"), Some(AgentState::Working));
        assert_eq!(state_from_title("◐ Refactor auth"), Some(AgentState::Working));
        assert_eq!(state_from_title("✳ Refactor auth"), Some(AgentState::Idle));
        assert_eq!(state_from_title("zsh"), None);
        assert_eq!(strip_title_glyph("✳ Refactor auth").as_deref(), Some("Refactor auth"));
    }

    #[test]
    fn done_until_focused() {
        let mut d = Detector::new(Path::new("/nonexistent"));
        assert_eq!(d.with_done("p", AgentState::Working, false), AgentState::Working);
        assert_eq!(d.with_done("p", AgentState::Idle, false), AgentState::Done);
        assert_eq!(d.with_done("p", AgentState::Idle, false), AgentState::Done);
        assert_eq!(d.with_done("p", AgentState::Idle, true), AgentState::Idle);
        assert_eq!(d.with_done("p", AgentState::Idle, false), AgentState::Idle);
    }

    /// Spawns a real process named `claude` under a shell and checks the detector finds it
    /// and reads its session file.
    #[cfg(unix)]
    #[test]
    fn finds_claude_child_and_reads_session_file() {
        let tmp = std::env::temp_dir().join(format!("wings-detect-{}", std::process::id()));
        fs::create_dir_all(tmp.join("sessions")).unwrap();
        let fake = tmp.join("claude");
        // A symlink, not a copy: macOS kills a copied platform binary on launch.
        let _ = fs::remove_file(&fake);
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();

        let mut shell = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("'{}' 30; true", fake.display()))
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let mut d = Detector::new(&tmp);
        let probe = PaneProbe {
            pane_id: "p1".into(),
            space_id: "s1".into(),
            shell_pid: Some(shell.id()),
            foreground_pid: None,
            title: String::new(),
        };
        let scan = d.scan(std::slice::from_ref(&probe), None);
        assert_eq!(scan.panes["p1"].command, "claude");
        let agents = scan.agents;
        assert_eq!(agents.len(), 1, "claude child not found");
        let pid = agents[0].pid;

        fs::write(
            tmp.join("sessions").join(format!("{pid}.json")),
            r#"{"sessionId":"abc","status":"waiting","waitingFor":"permission prompt","name":"Fix bug"}"#,
        )
        .unwrap();
        let agents = d.scan(std::slice::from_ref(&probe), None).agents;
        assert_eq!(agents[0].session_id.as_deref(), Some("abc"));
        assert_eq!(agents[0].state, AgentState::Blocked);
        assert_eq!(agents[0].waiting_for.as_deref(), Some("permission prompt"));
        assert_eq!(agents[0].name.as_deref(), Some("Fix bug"));

        shell.kill().unwrap();
        let _ = Command::new("pkill").arg("-f").arg(fake.to_str().unwrap()).status();
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The Unix path: the PTY's foreground process is `claude`, found without a full process scan.
    #[cfg(unix)]
    #[test]
    fn finds_claude_as_pty_foreground_process() {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        let tmp = std::env::temp_dir().join(format!("wings-fg-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("claude");
        let _ = fs::remove_file(&fake);
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();

        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", &format!("exec '{}' 30", fake.display())]);
        cmd.cwd(&tmp);
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let probe = PaneProbe {
            pane_id: "p1".into(),
            space_id: "s1".into(),
            shell_pid: child.process_id(),
            foreground_pid: pair.master.process_group_leader().map(|p| p as u32),
            title: String::new(),
        };
        let scan = Detector::new(&tmp).scan(std::slice::from_ref(&probe), None);
        assert_eq!(scan.panes["p1"].command, "claude");
        let cwd = std::path::PathBuf::from(scan.panes["p1"].cwd.clone().expect("cwd")).canonicalize().unwrap();
        assert_eq!(cwd, tmp.canonicalize().unwrap());
        assert_eq!(scan.agents.len(), 1);
        assert_eq!(Some(scan.agents[0].pid), probe.foreground_pid);

        child.kill().unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The flags come from the live process's argv, cleaned for a resume.
    #[cfg(unix)]
    #[test]
    fn reads_launch_flags_of_foreground_claude() {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        let tmp = std::env::temp_dir().join(format!("wings-args-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("claude");
        let _ = fs::remove_file(&fake);
        // bash runs `-c` and ignores the arguments after it, so it can stand in for `claude` with any flags.
        std::os::unix::fs::symlink("/bin/bash", &fake).unwrap();

        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(&fake);
        cmd.args(["-c", "sleep 30; true", "--model", "opus", "--append-system-prompt", "say \"hi\", don't ask", "--resume", "abc", "fix it"]);
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let probe = PaneProbe {
            pane_id: "p1".into(),
            space_id: "s1".into(),
            shell_pid: child.process_id(),
            foreground_pid: pair.master.process_group_leader().map(|p| p as u32),
            title: String::new(),
        };
        let agents = Detector::new(&tmp).scan(std::slice::from_ref(&probe), None).agents;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].args, ["--model", "opus", "--append-system-prompt", "say \"hi\", don't ask"]);

        child.kill().unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }
}
