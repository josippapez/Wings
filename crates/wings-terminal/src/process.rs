use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};

use crate::shell::{pane_command, pane_env};

/// Owns a PTY and its child. The host owns the only output read loop.
pub struct PtyProcess {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
    pid: Option<u32>,
    exit: Arc<Mutex<Option<std::io::Result<ExitStatus>>>>,
}

impl PtyProcess {
    pub fn spawn(cwd: &Path, pane_id: &str, cols: u16, rows: u16) -> anyhow::Result<Self> {
        Self::spawn_command(pane_command(cwd, pane_env(), pane_id), PtySize { rows, cols, ..PtySize::default() })
    }

    pub fn spawn_command(command: CommandBuilder, size: PtySize) -> anyhow::Result<Self> {
        let pair = native_pty_system().openpty(size)?;
        // Take the writer before spawning so a handle failure cannot orphan a child.
        let writer = pair.master.take_writer()?;
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        Ok(Self {
            pid: child.process_id(),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Arc::new(Mutex::new(Some(child))),
            exit: Arc::new(Mutex::new(None)),
        })
    }

    pub fn process_id(&self) -> Option<u32> {
        self.pid
    }

    /// The folder the shell is in now, which follows its `cd`. `None` once it has exited, and on
    /// platforms other than macOS and Linux.
    pub fn cwd(&self) -> Option<PathBuf> {
        let pid = self.pid?;
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStringExt;
            let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
            let read = unsafe {
                libc::proc_pidinfo(
                    pid as libc::c_int,
                    libc::PROC_PIDVNODEPATHINFO,
                    0,
                    (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                    size,
                )
            };
            if read != size {
                return None;
            }
            let path: Vec<u8> =
                info.pvi_cdir.vip_path.iter().flatten().take_while(|c| **c != 0).map(|c| *c as u8).collect();
            (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsString::from_vec(path)))
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = pid;
            None
        }
    }

    pub fn reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        self.master.lock().unwrap().try_clone_reader()
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.lock().unwrap().resize(PtySize { rows, cols, ..PtySize::default() })
    }

    #[cfg(unix)]
    pub fn foreground_pid(&self) -> Option<u32> {
        self.master.lock().unwrap().process_group_leader().map(|pid| pid as u32)
    }

    #[cfg(not(unix))]
    pub fn foreground_pid(&self) -> Option<u32> {
        None
    }

    /// Ends the pane and reaps its child off the calling thread.
    pub fn kill(&self) {
        self.finish(true);
    }

    /// Call at reader EOF to reap a natural exit without changing its exit status.
    #[cfg(unix)]
    pub fn reap(&self) {
        let Some(pid) = self.pid else { return };
        let (child, exit) = (self.child.clone(), self.exit.clone());
        thread::spawn(move || {
            // A child can close the PTY and keep running, so wait for the exit without reaping it.
            // The handle stays here until then, so kill and drop can still end the child.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            while unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) } == -1
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {}
            // None when kill took the child first. It reaps the child itself.
            let Some(mut child) = child.lock().unwrap().take() else { return };
            *exit.lock().unwrap() = Some(child.wait());
        });
    }

    #[cfg(not(unix))]
    pub fn reap(&self) {
        self.finish(false);
    }

    fn finish(&self, terminate: bool) {
        let Some(mut child) = self.child.lock().unwrap().take() else { return };
        let exit = self.exit.clone();
        thread::spawn(move || {
            // portable-pty's kill can include a grace period. Neither it nor wait runs on the UI.
            let result = if terminate {
                match child.try_wait() {
                    Ok(Some(status)) => Ok(status),
                    _ => {
                        let _ = child.kill();
                        child.wait()
                    }
                }
            } else {
                child.wait()
            };
            *exit.lock().unwrap() = Some(result);
        });
    }

    /// None until kill/EOF cleanup has reaped the child. Never waits for a running process.
    pub fn exit_status(&self) -> std::io::Result<Option<ExitStatus>> {
        match self.exit.lock().unwrap().as_ref() {
            Some(Ok(status)) => Ok(Some(status.clone())),
            Some(Err(error)) => Err(std::io::Error::new(error.kind(), error.to_string())),
            None => Ok(None),
        }
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::{self, Receiver},
        time::{Duration, Instant},
    };

    const TIMEOUT: Duration = Duration::from_secs(10);

    struct Session {
        process: PtyProcess,
        output: Receiver<Vec<u8>>,
        done: Receiver<()>,
        reader: Option<thread::JoinHandle<()>>,
    }

    impl Session {
        fn new(process: PtyProcess) -> Self {
            let mut reader = process.reader().unwrap();
            let (tx, output) = mpsc::channel();
            let (finished, done) = mpsc::channel();
            let reader = thread::spawn(move || {
                let mut buf = [0; 4096];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() { break; }
                }
                let _ = finished.send(());
            });
            Self { process, output, done, reader: Some(reader) }
        }

        fn until(&self, marker: &str) -> Vec<u8> {
            let deadline = Instant::now() + TIMEOUT;
            let mut bytes = Vec::new();
            while !String::from_utf8_lossy(&bytes).contains(marker) {
                bytes.extend(self.output.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("PTY output timed out"));
            }
            bytes
        }

        fn reaped(&self) -> ExitStatus {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                if let Some(status) = self.process.exit_status().unwrap() { return status; }
                assert!(Instant::now() < deadline, "child {:?} was not reaped", self.process.process_id());
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn join_reader(&mut self) {
            if self.reader.is_some() && self.done.recv_timeout(TIMEOUT).is_ok() {
                self.reader.take().unwrap().join().unwrap();
            }
            assert!(self.reader.is_none(), "PTY reader did not exit for {:?}", self.process.process_id());
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            self.process.kill();
            if self.reader.is_some() {
                if self.done.recv_timeout(TIMEOUT).is_ok() {
                    let _ = self.reader.take().unwrap().join();
                } else {
                    eprintln!("PTY reader cleanup timed out for {:?}", self.process.process_id());
                }
            }
        }
    }

    #[test]
    fn real_shell_preserves_environment_input_resize_and_exit_status() {
        let process = PtyProcess::spawn(&std::env::temp_dir(), "shared-test", 80, 24).unwrap();
        let mut session = Session::new(process);
        assert!(session.process.process_id().is_some());
        session.process.write(b"printf '\\nshared-''env:%s:%s:%s:%s:%s\\n' \"$TERM\" \"$COLORTERM\" \"$TERM_PROGRAM\" \"$WINGS_ENV\" \"$WINGS_PANE_ID\"; stty size\r").unwrap();
        let output = session.until("24 80");
        assert!(String::from_utf8_lossy(&output).contains("shared-env:xterm-256color:truecolor:Wings:1:shared-test"));
        assert!(session.process.foreground_pid().is_some());
        session.process.resize(92, 31).unwrap();
        session.process.write(b"printf '\\nresized-''size:'; stty size\r").unwrap();
        session.until("resized-size:31 92");
        session.process.write(b"read -r value; printf '\\nshared-''input:%s\\n' \"$value\"\rhello-world\r").unwrap();
        session.until("shared-input:hello-world");
        session.process.write(b"exit 7\r").unwrap();
        session.join_reader();
        session.process.reap();
        assert_eq!(session.reaped().exit_code(), 7);
    }

    #[test]
    fn kill_returns_without_waiting_for_a_shell_which_ignores_hangup_and_reaps_it() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; printf READY; read value"]);
        let mut session = Session::new(PtyProcess::spawn_command(command, PtySize::default()).unwrap());
        session.until("READY");
        let start = Instant::now();
        session.process.kill();
        assert!(start.elapsed() < Duration::from_millis(150), "kill blocked the caller");
        assert!(!session.reaped().success());
        session.join_reader();
        // Cleanup is idempotent even after a child was reaped.
        session.process.kill();
    }

    #[test]
    fn dropping_the_process_kills_and_reaps_the_child() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "read value"]);
        let process = PtyProcess::spawn_command(command, PtySize::default()).unwrap();
        let exit = process.exit.clone();
        let pid = process.process_id();
        drop(process);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = exit.lock().unwrap().as_ref() {
                assert!(status.is_ok(), "reaping dropped child {pid:?} failed");
                break;
            }
            assert!(Instant::now() < deadline, "dropped child {pid:?} was not reaped");
            thread::sleep(Duration::from_millis(10));
        }
    }

    struct EofCleanup {
        pid: u32,
        exit: Arc<Mutex<Option<std::io::Result<ExitStatus>>>>,
        armed: bool,
    }

    impl Drop for EofCleanup {
        fn drop(&mut self) {
            if !self.armed { return; }
            let mut kill = std::process::Command::new("/bin/kill")
                .args(["-KILL", &self.pid.to_string()]).spawn().unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while kill.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    let _ = std::process::Child::kill(&mut kill);
                    let _ = kill.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.exit.lock().unwrap().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if self.exit.lock().unwrap().is_none() { eprintln!("test child {} still needs cleanup", self.pid); }
        }
    }

    // On Linux a child that closes its terminal and ignores HUP gives the reader EOF while it keeps
    // running. macOS keeps the terminal open for the session leader, so call reap directly.
    fn eof_before_exit() -> (Session, EofCleanup) {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; printf EOF_READY; while :; do sleep 1; done"]);
        let session = Session::new(PtyProcess::spawn_command(command, PtySize::default()).unwrap());
        let cleanup = EofCleanup { pid: session.process.process_id().unwrap(), exit: session.process.exit.clone(), armed: true };
        session.until("EOF_READY");
        session.process.reap();
        (session, cleanup)
    }

    fn assert_eof_child_reaped(cleanup: &mut EofCleanup) {
        let deadline = Instant::now() + TIMEOUT;
        while cleanup.exit.lock().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(cleanup.exit.lock().unwrap().is_some(), "HUP-ignoring child {} survived EOF and termination", cleanup.pid);
        cleanup.armed = false;
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn cwd_follows_the_shell_into_the_folder_it_changed_to() {
        let start = std::env::temp_dir().canonicalize().unwrap();
        let target = start.join(format!("wings-terminal-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&target).unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.cwd(&start);
        command.args(["-c", "printf START; read go; cd \"$0\" && printf MOVED; sleep 30", target.to_str().unwrap()]);
        let session = Session::new(PtyProcess::spawn_command(command, PtySize::default()).unwrap());
        session.until("START");
        assert_eq!(session.process.cwd().unwrap().canonicalize().unwrap(), start);
        session.process.write(b"go\r").unwrap();
        session.until("MOVED");
        assert_eq!(session.process.cwd().unwrap().canonicalize().unwrap(), target);
        drop(session);
        std::fs::remove_dir(&target).unwrap();
    }

    #[test]
    fn kill_after_eof_reaps_a_hup_ignoring_child() {
        let (session, mut cleanup) = eof_before_exit();
        let start = Instant::now();
        session.process.kill();
        assert!(start.elapsed() < Duration::from_millis(150), "kill blocked the caller");
        assert_eof_child_reaped(&mut cleanup);
    }

    #[test]
    fn drop_after_eof_reaps_a_hup_ignoring_child() {
        let (session, mut cleanup) = eof_before_exit();
        drop(session);
        assert_eof_child_reaped(&mut cleanup);
    }
}
