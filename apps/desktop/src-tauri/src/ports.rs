//! Which local ports each pane's programs listen on, for plugins with `permissions.ports`. The process tree
//! under each pane's shell comes from sysinfo, like Claude detection. The sockets come from `lsof` with fixed
//! arguments (about 20 ms), because sysinfo doesn't read them and `lsof` ships with macOS and most Linux.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// One program listening for TCP connections that `localhost` reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanePort {
    pub pane_id: String,
    pub pid: u32,
    /// The program, like `node` or `Python`.
    pub name: String,
    pub port: u16,
    /// When the program started, in ms since the epoch.
    pub started_at: u64,
}

/// The listening sockets of every pane's shell and everything it started, by pane id and shell pid.
pub fn scan(panes: &[(String, Option<u32>)]) -> Result<Vec<PanePort>, String> {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let children = crate::detect::children_by_parent(system.processes());
    let mut owner: BTreeMap<u32, &str> = BTreeMap::new();
    for (pane_id, shell) in panes {
        let Some(shell) = shell else { continue };
        let shell = Pid::from_u32(*shell);
        for pid in std::iter::once(shell).chain(crate::detect::descendants(&children, shell)) {
            owner.entry(pid.as_u32()).or_insert(pane_id);
        }
    }
    if owner.is_empty() {
        return Ok(Vec::new());
    }
    let pids: Vec<u32> = owner.keys().copied().collect();
    let mut found: Vec<PanePort> = parse_lsof(&lsof(&pids)?)
        .into_iter()
        .filter_map(|(pid, command, port)| {
            let process = system.process(Pid::from_u32(pid));
            let name = process.map(|p| p.name().to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or(command);
            Some(PanePort { pane_id: owner.get(&pid)?.to_string(), pid, name, port, started_at: process.map_or(0, |p| p.start_time() * 1000) })
        })
        .collect();
    found.sort_by(|a, b| (&a.pane_id, a.port, a.pid).cmp(&(&b.pane_id, b.port, b.pid)));
    Ok(found)
}

#[cfg(unix)]
fn lsof(pids: &[u32]) -> Result<String, String> {
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let output = Command::new("lsof")
        .args(["-nP", "-a", "-p", &list, "-iTCP", "-sTCP:LISTEN", "-F", "pcn"])
        .output()
        .map_err(|e| format!("lsof isn't available: {e}"))?;
    // lsof exits 1 when nothing matched, which is the normal case, so only its output counts.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(unix))]
fn lsof(_pids: &[u32]) -> Result<String, String> {
    Err("Listening ports aren't read on this system yet".into())
}

/// `lsof -F pcn` output as (pid, command, port), once per pid and port. Only sockets that `localhost` reaches:
/// bound to every address, or to a loopback one.
fn parse_lsof(output: &str) -> Vec<(u32, String, u16)> {
    let mut found = BTreeSet::new();
    let (mut pid, mut command) = (None, String::new());
    for line in output.lines() {
        let Some(field) = line.chars().next() else { continue };
        let rest = &line[field.len_utf8()..];
        match field {
            'p' => (pid, command) = (rest.parse::<u32>().ok(), String::new()),
            'c' => command = rest.to_string(),
            'n' => {
                if let (Some(pid), Some(port)) = (pid, local_port(rest)) {
                    found.insert((pid, command.clone(), port));
                }
            }
            _ => {}
        }
    }
    found.into_iter().collect()
}

/// The port of `host:port` when the host is every address or a loopback one: `*`, `0.0.0.0`, `[::]`,
/// `127.x.x.x` and `[::1]`. A socket on one LAN address isn't promised to answer on `localhost`.
fn local_port(name: &str) -> Option<u16> {
    let (host, port) = name.rsplit_once(':')?;
    let reachable = matches!(host, "*" | "0.0.0.0" | "[::]" | "[::1]") || host.starts_with("127.");
    if !reachable {
        return None;
    }
    port.parse().ok().filter(|p| *p != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_listening_sockets_once_each() {
        let out = "p100\ncnode\nf21\nn*:5173\nf22\nn*:5173\nf23\nn[::1]:5173\np200\ncPython\nf3\nn127.0.0.1:8765\np300\ncpostgres\nf7\nn[::1]:15432\nf8\nn127.0.0.1:15432\n";
        assert_eq!(parse_lsof(out), [(100, "node".into(), 5173), (200, "Python".into(), 8765), (300, "postgres".into(), 15432)]);
    }

    #[test]
    fn skips_sockets_localhost_does_not_reach() {
        let out = "p1\ncweb\nn192.168.1.5:3000\nn[fe80::1]:3001\nn10.0.0.2:3002\nn127.0.0.1:3003\n";
        assert_eq!(parse_lsof(out), [(1, "web".into(), 3003)]);
        assert_eq!(local_port("*:0"), None);
        assert_eq!(local_port("*:notaport"), None);
        assert_eq!(local_port("[::]:80"), Some(80));
        assert_eq!(local_port("garbage"), None);
    }

    #[test]
    fn ignores_sockets_before_a_process_line() {
        assert!(parse_lsof("n*:80\ncx\n").is_empty());
        assert!(parse_lsof("").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn finds_a_listener_started_from_a_pane_shell() {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        // A shell whose job listens, the way a pane's shell starts a dev server. `; true` keeps sh from exec'ing it.
        let script = "import socket,sys;s=socket.socket();s.bind(('127.0.0.1',0));s.listen(1);print(s.getsockname()[1],flush=True);sys.stdin.read()";
        let mut shell = Command::new("sh").args(["-c", &format!("python3 -c \"{script}\"; true")]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(shell.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let port: u16 = line.trim().parse().unwrap();
        let found = scan(&[("other".into(), Some(1)), ("pane".into(), Some(shell.id()))]);
        drop(shell.stdin.take());
        let _ = shell.wait();
        let found = found.unwrap();
        let hit = found.iter().find(|p| p.port == port).expect("the listener is found");
        assert_eq!(hit.pane_id, "pane");
        assert!(hit.started_at > 0 && !hit.name.is_empty(), "{hit:?}");
    }
}
