use super::protocol::{
    self, Action, Error, PLUGIN_ID, Project, Request, Screen, StorageSnapshot, VERSION,
};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const WATCHDOG: Duration = Duration::from_secs(5);

/// A fixed, packaged sibling executable. No installed plugin or PATH lookup is accepted.
pub struct Broker {
    worker: PathBuf,
}

impl Broker {
    pub fn new() -> Result<Self, Error> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or(Error::Invalid("Missing worker directory"))?;
        Ok(Self {
            worker: directory.join(format!(
                "wings-plugin-worker{}",
                std::env::consts::EXE_SUFFIX
            )),
        })
    }

    /// Blocking: run off the UI thread. The host supplies only this project's selected storage namespace.
    pub fn invoke(
        &self,
        project: Project,
        storage: StorageSnapshot,
        action: Option<Action>,
    ) -> Result<Screen, Error> {
        let request = Request {
            version: VERSION,
            plugin_id: PLUGIN_ID.into(),
            project,
            storage,
            action,
        };
        request.validate()?;
        let payload = protocol::encode(&request)?;
        if !self.worker.is_absolute() {
            return Err(Error::Invalid("Worker path must be absolute"));
        }
        let child = Command::new(&self.worker)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let response = supervise(child, WATCHDOG, move |mut stdin, mut stdout| {
            protocol::write_frame(&mut stdin, &payload)?;
            drop(stdin);
            protocol::read_frame(&mut stdout)
        })?;
        let screen: Screen = serde_json::from_slice(&response)?;
        screen.validate()?;
        Ok(screen)
    }
}

struct Reap(Child);

impl Reap {
    fn stop(&mut self) -> std::io::Result<()> {
        // The response is terminal. Never trust even an EOF-producing child to exit voluntarily.
        let killed = self.0.kill();
        self.0.wait()?;
        killed
    }
}

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn supervise<T: Send + 'static>(
    child: Child,
    deadline: Duration,
    exchange: impl FnOnce(ChildStdin, ChildStdout) -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let mut child = Reap(child);
    let stdin = child.0.stdin.take().ok_or(Error::Worker)?;
    let stdout = child.0.stdout.take().ok_or(Error::Worker)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let io = thread::Builder::new()
        .name("plugin-pipes".into())
        .spawn(move || {
            let result = exchange(stdin, stdout);
            let _ = sender.send(result);
        })?;
    let result = match receiver.recv_timeout(deadline) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Deadline),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Worker),
    };
    let reaped = child.stop();
    let joined = io.join();
    reaped?;
    joined.map_err(|_| Error::Worker)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Instant;

    // This test binary is the only fixture executable. It cannot run through the production Broker API.
    fn fixture(name: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--ignored", "--nocapture"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    #[ignore]
    fn child_blocks_on_stdin() {
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        // Remain alive after stdin closes, without a timer, host API, or busy loop.
        child_never_reads();
    }

    #[test]
    #[ignore]
    fn child_never_reads() {
        let (_sender, receiver) = mpsc::channel::<()>();
        let _ = receiver.recv();
    }

    #[test]
    fn watchdog_terminates_and_reaps_blocked_pipe_io() {
        let start = Instant::now();
        let child = fixture("plugin::broker::tests::child_blocks_on_stdin");
        let pid = child.id();
        let result = supervise(child, Duration::from_millis(100), |stdin, mut stdout| {
            let _stdin = stdin;
            let mut output = Vec::new();
            stdout.read_to_end(&mut output)?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::Deadline)));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_reaped(pid);
    }

    #[test]
    fn watchdog_also_covers_blocked_writes() {
        let child = fixture("plugin::broker::tests::child_never_reads");
        let pid = child.id();
        let result = supervise(child, Duration::from_millis(100), |mut stdin, _stdout| {
            stdin.write_all(&vec![0; protocol::MAX_FRAME_BYTES])?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::Deadline)));
        assert_reaped(pid);
    }

    #[test]
    fn malformed_response_cleans_up_child() {
        let child = fixture("plugin::broker::tests::child_blocks_on_stdin");
        let pid = child.id();
        let result = supervise(child, Duration::from_secs(2), |_stdin, mut stdout| {
            protocol::read_frame(&mut stdout)
        });
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_reaped(pid);
    }

    #[test]
    fn pipe_thread_failure_cleans_up_child() {
        let child = fixture("plugin::broker::tests::child_never_reads");
        let pid = child.id();
        let result = supervise::<()>(child, Duration::from_secs(2), |_stdin, _stdout| {
            panic!("fixture failure")
        });
        assert!(matches!(result, Err(Error::Worker)));
        assert_reaped(pid);
    }

    #[cfg(unix)]
    fn assert_reaped(pid: u32) {
        // ps is used only to observe the test child, never to run plugin code.
        let output = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .output()
            .unwrap();
        assert!(output.stdout.is_empty(), "worker must be killed and reaped");
    }

    #[cfg(not(unix))]
    fn assert_reaped(_pid: u32) {}
}
