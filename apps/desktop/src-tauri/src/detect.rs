//! Finds Claude Code and opencode running inside Wings panes and works out what Claude is doing.
//! No Claude config changes: the process tree tells us a pane runs `claude` or `opencode`, Claude's own
//! `sessions/<pid>.json` gives the session id and status, and the OSC title is the fallback.
//! opencode's state and session come from its background service, see `opencode.rs`.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

mod launch_args;
use launch_args::launch_args;

use crate::opencode::{self, PaneRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    Working,
    Blocked,
    /// Finished a turn while you were looking elsewhere.
    Done,
    Idle,
}

/// Which agent CLI a pane runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentKind {
    Claude,
    Opencode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub pane_id: String,
    pub space_id: String,
    pub pid: u32,
    pub kind: AgentKind,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub state: AgentState,
    pub waiting_for: Option<String>,
    /// The flags `claude` was started with, to type again when Wings resumes the session. Empty for opencode.
    pub args: Vec<String>,
    /// When this `claude` process started, in ms since 1970. A resumed session's transcript also holds
    /// what came before it, and this tells the two apart.
    pub started_at: u64,
    /// When `state` last changed, in ms since 1970, from Claude Code's session file. For "working for 2m".
    pub state_since: Option<u64>,
    /// True when the agent's parent is the pane's own shell, so nothing like `npx` or `script` wraps it. Wings
    /// only puts an agent to sleep when it is, since the shell then goes back to its prompt once the agent exits.
    pub direct: bool,
    /// One line on what the agent just did, generated when a turn ends (see `ai.rs`). Not set by detection.
    pub summary: Option<String>,
    /// `name` came from the pane's terminal title, which says little: a generated name may replace it.
    #[serde(skip)]
    pub weak_name: bool,
}

/// Claude Code's live registry entry, `~/.claude/sessions/<pid>.json`. Undocumented, so every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
    pub name: Option<String>,
    pub status_updated_at: Option<u64>,
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
    opencode: std::sync::Arc<opencode::Shared>,
}

impl Detector {
    pub fn new(claude_dir: &Path) -> Self {
        Self {
            system: System::new(),
            sessions_dir: claude_dir.join("sessions"),
            last: HashMap::new(),
            unseen_done: HashMap::new(),
            traced: HashMap::new(),
            opencode: Default::default(),
        }
    }

    /// What the opencode threads read the panes from and write the state to.
    pub fn opencode(&self) -> std::sync::Arc<opencode::Shared> {
        self.opencode.clone()
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

        // Per pane: the process it is running now, and the agent process if that is one.
        type Found = (Option<Pid>, Option<(Pid, AgentKind)>);
        let found: Vec<Found> = panes
            .iter()
            .map(|pane| match (pane.foreground_pid.map(Pid::from_u32), pane.shell_pid.map(Pid::from_u32)) {
                (Some(fg), _) => (Some(fg), processes.get(&fg).and_then(|p| is_agent(p.name(), p.cmd())).map(|kind| (fg, kind))),
                (None, Some(shell)) => (Some(newest_child(&children, shell)), find_agent(processes, &children, shell)),
                (None, None) => (None, None),
            })
            .collect();
        // Read from the argv the scan above loaded to recognise the agent.
        let launch: Vec<(Vec<String>, u64)> = found
            .iter()
            .map(|(_, agent)| match agent.and_then(|(pid, kind)| processes.get(&pid).map(|p| (p, kind))) {
                Some((p, AgentKind::Claude)) => (launch_args(p.cmd()), p.start_time() * 1000),
                Some((p, AgentKind::Opencode)) => (Vec::new(), p.start_time() * 1000),
                None => Default::default(),
            })
            .collect();
        let direct: HashMap<String, bool> = panes
            .iter()
            .zip(&found)
            .filter_map(|(pane, (_, agent))| {
                let (pid, _) = (*agent)?;
                let parent = processes.get(&pid)?.parent().map(Pid::as_u32);
                Some((pane.pane_id.clone(), pane.shell_pid.is_some() && parent == pane.shell_pid))
            })
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
        let mut opencode_panes = Vec::new();
        for (((pane, (_, agent)), info), (args, started_at)) in panes.iter().zip(found).zip(infos).zip(launch) {
            let cwd = info.cwd.clone();
            pane_infos.insert(pane.pane_id.clone(), info);
            let Some((pid, kind)) = agent else {
                self.last.remove(&pane.pane_id);
                self.unseen_done.remove(&pane.pane_id);
                continue;
            };
            let (file, state) = match kind {
                AgentKind::Claude => {
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
                    (file, state)
                }
                AgentKind::Opencode => {
                    opencode_panes.push(PaneRef { pane_id: pane.pane_id.clone(), pid: pid.as_u32(), cwd, title: pane.title.clone() });
                    // Not the title: opencode's own title says nothing about what it is doing.
                    let view = self.opencode.view(&pane.pane_id);
                    // The service is gone for good: that is not a turn that finished.
                    if view.is_none() && self.opencode.link() == opencode::Link::Gone {
                        self.last.remove(&pane.pane_id);
                    }
                    let raw = match view.as_ref().map(|v| v.state) {
                        Some(opencode::State::Working) => AgentState::Working,
                        Some(opencode::State::Waiting) => AgentState::Blocked,
                        _ => AgentState::Idle,
                    };
                    let state = self.with_done(&pane.pane_id, raw, focused == Some(pane.pane_id.as_str()));
                    let file = view
                        .map(|v| SessionFile { session_id: Some(v.session_id), name: opencode::real_title(v.title), status_updated_at: Some(v.since), ..Default::default() })
                        .unwrap_or_default();
                    (file, state)
                }
            };
            let weak_name = kind == AgentKind::Claude && file.name.is_none();
            let name = match kind {
                AgentKind::Claude => file.name.or_else(|| strip_title_glyph(&pane.title)),
                AgentKind::Opencode => file.name,
            };
            agents.push(Agent {
                pane_id: pane.pane_id.clone(),
                space_id: pane.space_id.clone(),
                pid: pid.as_u32(),
                kind,
                session_id: file.session_id,
                name,
                state,
                waiting_for: if state == AgentState::Blocked { file.waiting_for } else { None },
                args,
                started_at,
                state_since: file.status_updated_at,
                direct: direct.get(&pane.pane_id).copied().unwrap_or(false),
                summary: None,
                weak_name,
            });
        }
        self.opencode.set_panes(opencode_panes);
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

/// Programs Claude Code and opencode start their MCP servers with.
const MCP_LAUNCHERS: &[&str] = &["node", "npm", "npx", "pnpm", "bun", "bunx", "deno", "uv", "uvx", "python", "python3"];

/// Whether a child of an agent process could be running work for it. Claude Code runs each Bash tool command,
/// background ones like a dev server included, as a child `zsh -c source …/shell-snapshots/…`; its other children are
/// MCP servers. Anything that is neither a known launcher nor named for MCP counts as work, so doubt reads as busy.
fn is_background_work(name: &str, cmd: &[OsString]) -> bool {
    let mcp = cmd.iter().any(|a| a.to_string_lossy().to_lowercase().contains("mcp"));
    !(mcp || MCP_LAUNCHERS.contains(&name))
}

/// True when `pid`, an agent, has a child that is running work (see `is_background_work`). A one-off read of the
/// process table, for the moment before Wings ends an idle agent; nothing calls it on a timer.
pub fn agent_busy(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
    let children = children_by_parent(system.processes());
    children.get(&Pid::from_u32(pid)).into_iter().flatten().any(|child| {
        let name = name_of(system.processes(), *child);
        let cmd = system.processes().get(child).map(|p| p.cmd().to_vec()).unwrap_or_default();
        is_background_work(&name, &cmd)
    })
}

/// A process keeps its pid when it execs another program, and sysinfo keeps the argv and name it cached. One
/// caught between a shell's fork and its exec of `claude` would read as the shell for good, so the processes
/// that can be a pane's job get their argv read again on every scan.
fn reread_argv(system: &mut System, pids: &[Pid]) {
    system.refresh_processes_specifics(ProcessesToUpdate::Some(pids), false, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
}

pub(crate) fn descendants(children: &HashMap<Pid, Vec<Pid>>, root: Pid) -> Vec<Pid> {
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

pub(crate) fn children_by_parent(processes: &HashMap<Pid, Process>) -> HashMap<Pid, Vec<Pid>> {
    let mut map: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, process) in processes {
        if let Some(parent) = process.parent() {
            map.entry(parent).or_default().push(*pid);
        }
    }
    map
}

/// Breadth-first, so the shallowest agent wins over one that Claude itself started.
fn find_agent(
    processes: &HashMap<Pid, Process>,
    children: &HashMap<Pid, Vec<Pid>>,
    root: Pid,
) -> Option<(Pid, AgentKind)> {
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        if pid != root {
            if let Some(kind) = processes.get(&pid).and_then(|p| is_agent(p.name(), p.cmd())) {
                return Some((pid, kind));
            }
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

/// What a process is, from its name and argv. opencode's Homebrew build is a native binary named `opencode`.
fn is_agent(name: &OsStr, cmd: &[OsString]) -> Option<AgentKind> {
    let argv0 = cmd.first().map(|a| Path::new(a).file_name().unwrap_or(a));
    let claude = name == "claude"
        || argv0 == Some(OsStr::new("claude"))
        // npm installs run as `node …/@anthropic-ai/claude-code/cli.js`
        || cmd.iter().any(|a| a.to_string_lossy().contains("@anthropic-ai/claude-code"));
    if claude {
        return Some(AgentKind::Claude);
    }
    (name == "opencode" || argv0 == Some(OsStr::new("opencode"))).then_some(AgentKind::Opencode)
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
    use std::{fs, process::Command, time::{Duration, Instant}};

    /// Refreshes `pid` in `system` with `kind` until its name is `want`, or until 10 s pass. Returns the last name read.
    fn name_when(system: &mut System, pid: Pid, kind: ProcessRefreshKind, want: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            let name = name_of(system.processes(), pid);
            if name == want || Instant::now() >= deadline {
                return name;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn sees_a_process_that_becomes_another_program() {
        // `sh` waits, then execs `sleep`: the same pid, a new program. The wait is long, so reading `sh` is not
        // a race with the exec, and the reads below wait for each state instead of timing it.
        let mut child = Command::new("/bin/sh").args(["-c", "sleep 2; exec /bin/sleep 5"]).spawn().unwrap();
        let pid = Pid::from_u32(child.id());
        let mut system = System::new();
        let cached = ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet);
        let before = name_when(&mut system, pid, cached, "sh");
        // A fresh read showing `sleep` proves the exec happened; the cached view must still show `sh`.
        let mut fresh = System::new();
        let always = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always);
        let after_exec = name_when(&mut fresh, pid, always, "sleep");
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, cached);
        let stale = name_of(system.processes(), pid);
        reread_argv(&mut system, &[pid]);
        let after = name_of(system.processes(), pid);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(before, "sh");
        assert_eq!(after_exec, "sleep", "the process execs into sleep");
        assert_eq!(stale, "sh", "sysinfo keeps the argv it read before the exec");
        assert_eq!(after, "sleep");
    }

    #[test]
    fn recognises_claude_and_opencode_by_name_or_argv0() {
        let argv = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(is_agent(OsStr::new("claude"), &argv(&["claude"])), Some(AgentKind::Claude));
        assert_eq!(is_agent(OsStr::new("node"), &argv(&["/opt/homebrew/bin/claude"])), Some(AgentKind::Claude));
        assert_eq!(
            is_agent(OsStr::new("node"), &argv(&["node", "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js"])),
            Some(AgentKind::Claude)
        );
        assert_eq!(is_agent(OsStr::new("opencode"), &argv(&["opencode"])), Some(AgentKind::Opencode));
        assert_eq!(is_agent(OsStr::new("opencode"), &[]), Some(AgentKind::Opencode));
        assert_eq!(is_agent(OsStr::new("opencode-x"), &argv(&["/opt/homebrew/bin/opencode", "--port", "4096"])), Some(AgentKind::Opencode));
        assert_eq!(is_agent(OsStr::new("node"), &argv(&["node", "server.js"])), None);
        assert_eq!(is_agent(OsStr::new("zsh"), &argv(&["-zsh"])), None);
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
        assert!(agents[0].direct, "claude is the shell's own child");
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

    /// An opencode pane gets its session and state from `opencode.rs`, goes through "done" like Claude, and its
    /// terminal title never decides the state.
    #[cfg(unix)]
    #[test]
    fn opencode_agent_takes_state_and_session_from_the_service() {
        let tmp = std::env::temp_dir().join(format!("wings-oc-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("opencode");
        let _ = fs::remove_file(&fake);
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
        let mut shell = Command::new("/bin/sh").arg("-c").arg(format!("'{}' 30; true", fake.display())).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let mut d = Detector::new(&tmp);
        let probe = PaneProbe { pane_id: "p1".into(), space_id: "s1".into(), shell_pid: Some(shell.id()), foreground_pid: None, title: "✳ looks idle".into() };
        let view = |state, since| opencode::PaneView { session_id: "ses_a".into(), title: Some("Fix it".into()), state, since };
        let scan = |d: &mut Detector, focused| d.scan(std::slice::from_ref(&probe), focused).agents.remove(0);

        let first = scan(&mut d, None);
        assert_eq!((first.kind, first.state, first.session_id, first.name), (AgentKind::Opencode, AgentState::Idle, None, None));
        d.opencode().seed("p1", view(opencode::State::Working, 1000));
        let a = scan(&mut d, None);
        assert_eq!((a.state, a.session_id.as_deref(), a.name.as_deref(), a.state_since), (AgentState::Working, Some("ses_a"), Some("Fix it"), Some(1000)));
        d.opencode().seed("p1", view(opencode::State::Waiting, 2000));
        assert_eq!(scan(&mut d, None).state, AgentState::Blocked);
        d.opencode().seed("p1", view(opencode::State::Working, 3000));
        scan(&mut d, None);
        d.opencode().seed("p1", view(opencode::State::Idle, 4000));
        assert_eq!(scan(&mut d, None).state, AgentState::Done, "finished while the pane was out of sight");
        assert_eq!(scan(&mut d, Some("p1")).state, AgentState::Idle);

        // The service restarts: the last known state stays, so no "done" until the snapshot says so.
        d.opencode().seed("p1", view(opencode::State::Working, 5000));
        scan(&mut d, None);
        d.opencode().disconnected();
        assert_eq!(scan(&mut d, None).state, AgentState::Working);
        d.opencode().seed("p1", view(opencode::State::Idle, 6000));
        assert_eq!(scan(&mut d, None).state, AgentState::Done, "it finished while the service was away");
        assert_eq!(scan(&mut d, Some("p1")).state, AgentState::Idle);

        // The service never comes back: idle, not done.
        d.opencode().seed("p1", view(opencode::State::Working, 7000));
        scan(&mut d, None);
        d.opencode().gone();
        assert_eq!(scan(&mut d, None).state, AgentState::Idle);

        shell.kill().unwrap();
        let _ = shell.wait();
        let _ = Command::new("pkill").arg("-f").arg(fake.to_str().unwrap()).status();
        let _ = fs::remove_dir_all(&tmp);
    }

    /// An agent started through another process (here an inner `sh`) is not the shell's own child.
    #[cfg(unix)]
    #[test]
    fn agent_behind_a_wrapper_is_not_direct() {
        let tmp = std::env::temp_dir().join(format!("wings-wrap-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("claude");
        let _ = fs::remove_file(&fake);
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
        let mut shell = Command::new("/bin/sh").arg("-c").arg(format!("sh -c \"'{}' 30; true\"; true", fake.display())).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let probe = PaneProbe { pane_id: "p1".into(), space_id: "s1".into(), shell_pid: Some(shell.id()), foreground_pid: None, title: String::new() };
        let agents = Detector::new(&tmp).scan(std::slice::from_ref(&probe), None).agents;
        assert_eq!(agents.len(), 1);
        assert!(!agents[0].direct, "the inner sh sits between the shell and claude");

        shell.kill().unwrap();
        let _ = shell.wait();
        let _ = Command::new("pkill").arg("-f").arg(fake.to_str().unwrap()).status();
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Claude Code runs a Bash command as a child shell: that is work, while MCP servers and no children are not.
    #[cfg(unix)]
    #[test]
    fn agent_with_a_shell_child_is_busy() {
        let tmp = std::env::temp_dir().join(format!("wings-busy-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("claude");
        let _ = fs::remove_file(&fake);
        std::os::unix::fs::symlink("/bin/sh", &fake).unwrap();
        let run = |script: &str| Command::new(&fake).args(["-c", script]).spawn().unwrap();
        let wait_for = |pid: u32, want: bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while agent_busy(pid) != want && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            agent_busy(pid)
        };
        // `sleep 37; true` keeps the child a separate process; `sh -c` is the shell child.
        let mut busy = run("sh -c 'sleep 37'; true");
        let mut mcp = run("sh -c '/bin/sleep 37; true' standalone-mcp; true");
        let mut idle = Command::new("/bin/sleep").arg("37").spawn().unwrap();
        assert!(wait_for(busy.id(), true), "a shell child is background work");
        assert!(!wait_for(idle.id(), false), "no children is not busy");
        assert!(!agent_busy(mcp.id()), "an MCP server child is not work");
        for c in [&mut busy, &mut mcp, &mut idle] {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = Command::new("pkill").arg("-f").arg("sleep 37").status();
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tells_mcp_servers_from_work() {
        let argv = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(is_background_work("zsh", &argv(&["/bin/zsh", "-c", "source ~/.claude/shell-snapshots/x.sh && npm run dev"])));
        assert!(is_background_work("sh", &argv(&["sh", "-c", "sleep 30"])));
        assert!(is_background_work("something-unknown", &argv(&["something-unknown"])));
        assert!(!is_background_work("node", &argv(&["node", "/x/server.js"])));
        assert!(!is_background_work("npm", &argv(&["npm", "exec", "@some/server"])));
        assert!(!is_background_work("tool", &argv(&["tool", "/x/standalone-mcp.cjs"])));
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
