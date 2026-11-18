//! Programs a plugin keeps running and talks to, under `permissions.processes`: it writes to a program's input and
//! hears each line the program prints, as an agent run headless needs (`claude -p --input-format stream-json`).

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{
    exec::{allowing_entry, args_check, command, not_allowed},
    manifest::Plugin,
};

/// The longest line a program may print. One stream-json line holds a whole message, tool results and all.
pub const LINE_LIMIT: usize = 8 * 1024 * 1024;
pub const LINE_TOO_LONG: &str = "The program printed a line longer than 8 MB";
/// How much of what the program printed to stderr is kept, from the end, for when it stops.
const STDERR_KEPT: usize = 16 * 1024;
/// How long a program has to end once its input is closed, before it's killed. Claude ends at once.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Everything `run` checks before it starts `program` in `cwd`: the manifest's `processes` lists it, its arguments pass
/// `exec`'s checks, and after the words its entry names come only flags, each value joined to its flag
/// (`--resume=<id>`). A word of its own could be read as a subcommand whatever flag comes before it, like `mcp add` in
/// `claude --verbose mcp --verbose add`, which changes your Claude setup for good, and a wrapper like `claude-rtk` reads it
/// the same.
pub(crate) fn check(plugin: &Plugin, program: &str, args: &[String], cwd: &Path) -> Result<(), String> {
    let id = &plugin.manifest.id;
    let Some(named) = allowing_entry(&plugin.manifest.permissions.processes, program, args) else {
        return Err(not_allowed(plugin, program, args));
    };
    if let Some(word) = args[named..].iter().find(|arg| !arg.starts_with('-')) {
        return Err(format!("{id} may pass {program} only flags past its entry, each value joined as --flag=value, not {word:?}"));
    }
    args_check(plugin, program, args, Some(cwd))
}

/// How a program ended: its exit code (none when a signal ended it) and the end of what it printed to stderr.
#[derive(Debug, Serialize)]
pub struct Exit {
    pub code: Option<i32>,
    pub stderr: String,
}

/// Hears how a write went.
pub type Written = Box<dyn FnOnce(Result<(), String>) + Send>;

type Writes = mpsc::Receiver<(String, Written)>;

/// A program a plugin runs, from before it starts until it ends.
pub struct Process {
    /// Set once it has started.
    child: Mutex<Option<Child>>,
    /// Where writes wait for the writer, which closes the program's input once this is dropped.
    input: Mutex<Option<mpsc::Sender<(String, Written)>>>,
    /// The writes, until the writer takes them when the program starts.
    writes: Mutex<Option<Writes>>,
}

impl Process {
    /// A program yet to start: what is written to it now reaches it once `run` has started it.
    pub fn new() -> Arc<Self> {
        let (input, writes) = mpsc::channel();
        Arc::new(Self { child: Mutex::new(None), input: Mutex::new(Some(input)), writes: Mutex::new(Some(writes)) })
    }

    /// Writes `text` to the program's input as it is (a line needs its own `\n`), after the writes before it, and
    /// tells `done` how that went. Never waits: a program that isn't reading holds up only its own writes.
    pub fn write(&self, text: String, done: impl FnOnce(Result<(), String>) + Send + 'static) {
        let done: Written = Box::new(done);
        let refused = match &*self.input.lock().unwrap() {
            Some(input) => input.send((text, done)).err().map(|mpsc::SendError((_, done))| done),
            None => Some(done),
        };
        if let Some(done) = refused {
            done(Err(INPUT_CLOSED.into()));
        }
    }

    /// The program's process id while it runs, so Wings can tell which page started a program that calls it.
    pub fn pid(&self) -> Option<u32> {
        self.child.lock().unwrap().as_ref().map(Child::id)
    }

    /// Closes the program's input once the writes before have gone, which tells a program like Claude to finish, and
    /// kills it if it's still running `STOP_GRACE` later. Never waits.
    pub fn stop(self: &Arc<Self>) {
        if self.input.lock().unwrap().take().is_none() {
            return;
        }
        let this = self.clone();
        thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < STOP_GRACE {
                // One not started yet starts with its input closed, and ends by itself.
                let running = this.child.lock().unwrap().as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_none());
                if !running {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            this.kill();
        });
    }

    fn kill(&self) {
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }

    /// For a program that didn't start: closes its input and fails the writes that were waiting for it.
    fn close(&self) {
        // The input first, so that no write comes in after the waiting ones are answered.
        self.input.lock().unwrap().take();
        if let Some(writes) = self.writes.lock().unwrap().take() {
            for (_, done) in writes.try_iter() {
                done(Err(INPUT_CLOSED.into()));
            }
        }
    }
}

const INPUT_CLOSED: &str = "The program's input is closed";

/// Writes what `writes` hands it, in order, until the sender is dropped, and then closes the program's input. Every write
/// is answered: after one fails, the rest fail too.
fn writer(mut stdin: ChildStdin, writes: Writes) {
    let mut broken: Option<String> = None;
    for (text, done) in writes {
        let result = match &broken {
            Some(error) => Err(error.clone()),
            None => stdin.write_all(text.as_bytes()).and_then(|()| stdin.flush()).map_err(|error| format!("{INPUT_CLOSED}: {error}")),
        };
        if let Err(error) = &result {
            broken = Some(error.clone());
        }
        done(result);
    }
}

/// Runs `process` as `program` with `args` in `cwd`, once `check` allows it. `on_line` hears each line it prints to stdout, in order and without its line ending; when it returns
/// false nobody listens any more, and the program is stopped. Returns once the program's output has closed and it has
/// ended.
pub fn run(
    plugin: &Plugin,
    process: &Arc<Process>,
    program: &str,
    args: &[String],
    cwd: &Path,
    on_line: impl FnMut(&str) -> bool,
) -> Result<Exit, String> {
    run_under(plugin, process, program, args, cwd, None, on_line)
}

/// `run` with `CLAUDE_CONFIG_DIR` set to `config_dir`, the Claude Code account the program runs under. The host picks it
/// from an account it knows; a plugin never names the folder.
pub fn run_under(
    plugin: &Plugin,
    process: &Arc<Process>,
    program: &str,
    args: &[String],
    cwd: &Path,
    config_dir: Option<&Path>,
    mut on_line: impl FnMut(&str) -> bool,
) -> Result<Exit, String> {
    let (stdout, stderr) = start(plugin, process, program, args, cwd, config_dir).inspect_err(|_| process.close())?;
    let errors = thread::spawn(move || tail(stderr));
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        line.clear();
        // One byte past the limit is read to tell a line that is too long.
        match (&mut reader).take(LINE_LIMIT as u64 + 1).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() > LINE_LIMIT => {
                too_long = true;
                process.kill();
                break;
            }
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        if !on_line(text.trim_end_matches(['\n', '\r'])) {
            process.stop();
            break;
        }
    }
    drop(reader);
    // Polled, not waited on, so that `stop` can still reach the child to kill it.
    let status = loop {
        let mut child = process.child.lock().unwrap();
        if let Some(status) = child.as_mut().unwrap().try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        drop(child);
        thread::sleep(Duration::from_millis(10));
    };
    process.input.lock().unwrap().take();
    let stderr = errors.join().unwrap_or_default();
    if too_long {
        return Err(LINE_TOO_LONG.into());
    }
    Ok(Exit { code: status.code(), stderr })
}

/// Checks the command, starts it with the writer on its input, and gives its output pipes.
fn start(plugin: &Plugin, process: &Process, program: &str, args: &[String], cwd: &Path, config_dir: Option<&Path>) -> Result<(ChildStdout, ChildStderr), String> {
    check(plugin, program, args, cwd)?;
    let mut command = command(program, args, Some(cwd))?;
    if let Some(dir) = config_dir {
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    let mut child = command.stdin(Stdio::piped()).spawn().map_err(|error| error.to_string())?;
    let (stdout, stderr, stdin) = (child.stdout.take().unwrap(), child.stderr.take().unwrap(), child.stdin.take().unwrap());
    if let Some(writes) = process.writes.lock().unwrap().take() {
        thread::spawn(move || writer(stdin, writes));
    }
    *process.child.lock().unwrap() = Some(child);
    Ok((stdout, stderr))
}

/// The last `STDERR_KEPT` bytes of the pipe, read to its end.
fn tail(mut pipe: ChildStderr) -> String {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n @ 1..) = pipe.read(&mut chunk) {
        kept.extend_from_slice(&chunk[..n]);
        if kept.len() > 2 * STDERR_KEPT {
            kept.drain(..kept.len() - STDERR_KEPT);
        }
    }
    let start = kept.len().saturating_sub(STDERR_KEPT);
    String::from_utf8_lossy(&kept[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::plugin;

    fn with_processes(entries: &[&str]) -> Plugin {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        p.manifest.permissions.processes = entries.iter().map(|s| s.to_string()).collect();
        p
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn only_listed_programs_run() {
        let p = with_processes(&["cat []"]);
        let refused = |program: &str, words: &[&str]| run(&p, &Process::new(), program, &args(words), Path::new("/tmp"), |_| true).unwrap_err();
        assert!(refused("sh", &["-c", "echo hi"]).contains("may not run sh"));
        assert!(refused("cat", &["-u"]).contains("may not run cat"));
        assert!(refused("cat", &["/etc/hosts"]).contains("only flags past its entry"));
        // Listed under exec isn't enough.
        let mut exec_only = plugin(Path::new("/tmp"), &["cat"], &[]);
        exec_only.manifest.permissions.processes.clear();
        assert!(run(&exec_only, &Process::new(), "cat", &[], Path::new("/tmp"), |_| true).unwrap_err().contains("may not run cat"));
    }

    #[test]
    fn past_its_entry_a_program_gets_only_flags_with_their_values_joined() {
        let p = with_processes(&["claude [-p --input-format --output-format --verbose --permission-prompt-tool --resume]", "claude-rtk [-p --verbose]"]);
        let tmp = Path::new("/tmp");
        let headless = args(&["-p", "--input-format=stream-json", "--permission-prompt-tool=stdio", "--resume=3c21b0c5"]);
        assert_eq!(check(&p, "claude", &headless, tmp), Ok(()));
        for stray in [&["mcp", "add", "x", "sh"][..], &["--verbose", "mcp", "--verbose", "add"], &["--resume", "a"], &["-p", "prompt"]] {
            assert!(check(&p, "claude", &args(stray), tmp).unwrap_err().contains("only flags past its entry"), "{stray:?}");
        }
        assert!(check(&p, "claude-rtk", &args(&["-p", "mcp"]), tmp).unwrap_err().contains("only flags past its entry"));
        assert!(check(&p, "claude", &args(&["--mcp-config=x.json"]), tmp).unwrap_err().contains("may not run claude"));
        // Words the entry names are its own.
        assert_eq!(check(&with_processes(&["seq 1 3"]), "seq", &args(&["1", "3"]), tmp), Ok(()));
    }

    #[test]
    fn hears_each_line_in_order_of_what_it_was_written() {
        let p = with_processes(&["cat []"]);
        let mut heard = Vec::new();
        let (written, results) = mpsc::channel();
        let process = Process::new();
        // Written before it starts, in small writes each answered on its own: they still reach it, in order.
        for text in ["one\n", "{\"type\":", "\"user\"}\n", "three\r\n", "late\n"] {
            let written = written.clone();
            if text == "late\n" {
                process.stop();
            }
            process.write(text.into(), move |result| written.send((text, result)).unwrap());
        }
        let exit = run(
            &p,
            &process,
            "cat",
            &[],
            Path::new("/tmp"),
            |line| {
                heard.push(line.to_owned());
                true
            },
        )
        .unwrap();
        assert_eq!(heard, ["one", "{\"type\":\"user\"}", "three"]);
        assert_eq!(exit.code, Some(0));
        let results: Vec<_> = results.try_iter().collect();
        assert_eq!(results.len(), 5, "every write is answered");
        assert!(results.iter().all(|(text, result)| result.is_ok() == (*text != "late\n")), "{results:?}");
    }

    #[test]
    fn writes_to_a_program_that_could_not_start_are_answered() {
        let p = with_processes(&["cat []"]);
        let process = Process::new();
        let (written, results) = mpsc::channel();
        process.write("hi\n".into(), move |result| written.send(result).unwrap());
        assert!(run(&p, &process, "sh", &[], Path::new("/tmp"), |_| true).is_err());
        assert_eq!(results.try_recv().unwrap(), Err(INPUT_CLOSED.into()));
        process.write("again\n".into(), |result| assert!(result.is_err()));
    }

    #[test]
    fn a_program_nobody_listens_to_is_stopped() {
        let p = with_processes(&["yes"]);
        let start = Instant::now();
        let mut heard = 0;
        let exit = run(&p, &Process::new(), "yes", &[], Path::new("/tmp"), |_| {
            heard += 1;
            heard < 3
        })
        .unwrap();
        assert_eq!(heard, 3);
        assert_ne!(exit.code, Some(0));
        assert!(start.elapsed() < STOP_GRACE + Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn a_line_over_the_limit_stops_the_program() {
        let size = (LINE_LIMIT + 10).to_string();
        let p = with_processes(&[&format!("head -c {size} /dev/zero")]);
        let err = run(&p, &Process::new(), "head", &args(&["-c", &size, "/dev/zero"]), Path::new("/tmp"), |_| true).unwrap_err();
        assert_eq!(err, LINE_TOO_LONG);
    }

    #[test]
    fn keeps_the_end_of_stderr() {
        let script = std::env::temp_dir().join(format!("wings-stderr-{}.sh", std::process::id()));
        std::fs::write(&script, "i=0; while [ $i -lt 3000 ]; do echo line-$i >&2; i=$((i+1)); done; exit 3").unwrap();
        let script = script.to_string_lossy().into_owned();
        let p = with_processes(&[&format!("sh {script}")]);
        let exit = run(&p, &Process::new(), "sh", &args(&[&script]), Path::new("/tmp"), |_| true).unwrap();
        std::fs::remove_file(&script).unwrap();
        assert_eq!(exit.code, Some(3));
        assert!(exit.stderr.len() <= STDERR_KEPT);
        assert!(exit.stderr.ends_with("line-2999\n"), "{}", &exit.stderr[exit.stderr.len() - 20..]);
    }

    #[test]
    fn runs_under_the_account_folder_the_host_gives_it() {
        let script = std::env::temp_dir().join(format!("wings-account-{}.sh", std::process::id()));
        std::fs::write(&script, "echo \"dir:$CLAUDE_CONFIG_DIR\"").unwrap();
        let script = script.to_string_lossy().into_owned();
        let p = with_processes(&[&format!("sh {script}")]);
        let heard = |config: Option<&Path>| {
            let mut lines = Vec::new();
            run_under(&p, &Process::new(), "sh", &args(&[&script]), Path::new("/tmp"), config, |line| {
                lines.push(line.to_owned());
                true
            })
            .unwrap();
            lines
        };
        assert_eq!(heard(Some(Path::new("/accounts/work"))), ["dir:/accounts/work"]);
        // Without one the program keeps Wings' own environment.
        assert_eq!(heard(None), [format!("dir:{}", std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default())]);
        std::fs::remove_file(&script).unwrap();
    }
}
