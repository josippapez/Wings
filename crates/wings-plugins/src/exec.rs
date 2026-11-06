//! Runs the programs a plugin may exec, with no shell, and finds them on the login shell's PATH.

use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{
    cli_rules::{api_endpoints_ok, az_reaches_azure_devops_only, command_refused, flag_not_taken, git_args_ok, is_unsafe_flag, jq_reads_env},
    manifest::Plugin,
};

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) const EXEC_TIMEOUT: Duration = Duration::from_secs(30);
/// Long enough for a browser sign-in (`az login`), short enough that nothing hangs forever.
const EXEC_TIMEOUT_MAX: Duration = Duration::from_secs(300);

pub fn exec_timeout(requested_ms: Option<u64>) -> Duration {
    requested_ms.map_or(EXEC_TIMEOUT, |ms| Duration::from_millis(ms).clamp(Duration::from_secs(1), EXEC_TIMEOUT_MAX))
}
const EXEC_OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
/// What a plugin's exec rejects with when a program prints more than `EXEC_OUTPUT_LIMIT` bytes to stdout or stderr.
pub const EXEC_OUTPUT_TOO_LARGE: &str = "Output is larger than 8 MB";

/// Gets each output line while the program runs, for prompts like a sign-in code.
pub type OnLine = Arc<dyn Fn(&str) + Send + Sync>;

/// Reads a pipe on its own thread so a chatty program can't block on a full pipe.
/// Returns the text and whether the program printed more than the limit. One byte past it is read to tell.
fn read_pipe(pipe: impl Read + Send + 'static, on_line: Option<OnLine>, overflow: Arc<AtomicBool>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe.take(EXEC_OUTPUT_LIMIT + 1));
        let (mut all, mut line) = (Vec::new(), Vec::new());
        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            if let Some(f) = &on_line {
                f(String::from_utf8_lossy(&line).trim_end());
            }
            all.append(&mut line);
        }
        if all.len() as u64 > EXEC_OUTPUT_LIMIT {
            overflow.store(true, Ordering::SeqCst);
        }
        String::from_utf8_lossy(&all).into_owned()
    })
}

/// An exec entry split into its command and, from a trailing `[...]`, the only flags it allows.
pub(crate) fn exec_entry(entry: &str) -> (&str, Option<Vec<&str>>) {
    match entry.split_once('[') {
        Some((command, flags)) => (command, Some(flags.trim_end_matches(']').split_whitespace().collect())),
        None => (entry, None),
    }
}

/// Whether a `permissions.exec` entry covers this call: `gh pr view` allows `gh pr view <url> --json ...`,
/// `az boards work-item update [--id --state]` allows that command with no flags but those two, and `gh pr diff []`
/// allows no flags at all.
pub(crate) fn may_exec(plugin: &Plugin, program: &str, args: &[String]) -> bool {
    plugin.manifest.permissions.exec.iter().any(|entry| {
        let (command, flags) = exec_entry(entry);
        let mut words = command.split_whitespace();
        words.next() == Some(program)
            && words.enumerate().all(|(i, word)| args.get(i).is_some_and(|a| a == word))
            // Exact names only: az would take `--stat` for `--state`, so an abbreviation counts as another flag.
            && flags.is_none_or(|flags| args.iter().filter(|a| a.starts_with('-')).all(|a| flags.contains(&a.split('=').next().unwrap_or(a))))
    })
}

/// Runs a command the manifest allows. No shell, so arguments can't smuggle in commands.
pub fn exec(plugin: &Plugin, program: &str, args: &[String], cwd: Option<&Path>, timeout: Duration, on_line: Option<OnLine>) -> Result<ExecResult, String> {
    exec_check(plugin, program, args, cwd)?;
    run(program, args, cwd, timeout, on_line)
}

/// Everything `exec` checks before it runs a command in `cwd`.
pub(crate) fn exec_check(plugin: &Plugin, program: &str, args: &[String], cwd: Option<&Path>) -> Result<(), String> {
    let id = &plugin.manifest.id;
    if !may_exec(plugin, program, args) {
        let sub = args.iter().take_while(|a| !a.starts_with('-')).take(3).cloned().collect::<Vec<_>>().join(" ");
        return Err(format!("{id} may not run {program} {sub}"));
    }
    command_refused(id, program, args)?;
    if let Some(flag) = args.iter().find(|a| is_unsafe_flag(program, a)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    if let Some(flag) = flag_not_taken(program, args) {
        return Err(format!("{id} may not pass {flag} to {program} {}", args[0]));
    }
    if program == "az" && !az_reaches_azure_devops_only(args) {
        return Err(format!("{id} may only reach Azure DevOps with az, with its token"));
    }
    if !api_endpoints_ok(program, args) {
        return Err(format!("{id} may only call {program} api with a plain API path"));
    }
    if program == "gh" && jq_reads_env(args) {
        return Err(format!("{id} may not read the environment with --jq"));
    }
    if program == "git" && !git_args_ok(args, cwd) {
        return Err(format!("{id} may only name paths inside the folder git runs in, and fetch no refspec with a destination"));
    }
    Ok(())
}

/// Runs a program once its arguments have been checked.
pub(crate) fn run(program: &str, args: &[String], cwd: Option<&Path>, timeout: Duration, on_line: Option<OnLine>) -> Result<ExecResult, String> {
    let path = find_program(program).ok_or_else(|| format!("{program} is not installed or not on PATH"))?;
    let mut cmd = Command::new(path);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A plugin's folder, or a branch it fetched and checked out, can hold a folder shaped like a bare repo, whose config
    // runs commands (`diff.external`, `remote.<name>.uploadpack`) when git, or the az, gh and glab that call git, start
    // inside it. git takes this setting only from config a repo can't write, like the command line's.
    cmd.env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", "safe.bareRepository").env("GIT_CONFIG_VALUE_0", "explicit");
    if program == "git" {
        // Makes git reject abbreviated options outright, on top of the prefix check above.
        cmd.env("GIT_TEST_DISALLOW_ABBREVIATED_OPTIONS", "1");
    }
    if let Some(dir) = cwd {
        if !dir.is_dir() {
            return Err(format!("{} is not a folder", dir.display()));
        }
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let overflow = Arc::new(AtomicBool::new(false));
    let out_reader = read_pipe(child.stdout.take().unwrap(), on_line.clone(), overflow.clone());
    let err_reader = read_pipe(child.stderr.take().unwrap(), on_line, overflow.clone());
    let start = Instant::now();
    let status = loop {
        // Checked before the exit status: a program cut off by the closed pipe dies by signal and would look like a failure.
        if overflow.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(EXEC_OUTPUT_TOO_LARGE.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(format!("{program} timed out after {}s", timeout.as_secs()));
        }
        // 2 ms: at 20 ms every exec paid ~22 ms of waiting after the program had already finished (measured on echo: 25 ms -> 3 ms).
        std::thread::sleep(Duration::from_millis(2));
    };
    let (stdout, stderr) = (out_reader.join().unwrap_or_default(), err_reader.join().unwrap_or_default());
    if overflow.load(Ordering::SeqCst) {
        return Err(EXEC_OUTPUT_TOO_LARGE.into());
    }
    Ok(ExecResult { code: status.code(), stdout, stderr })
}

/// GUI apps don't get the shell's PATH on macOS and Linux, so ask the login shell, until it answers.
pub fn search_path() -> String {
    static CACHE: std::sync::Mutex<ShellPath> = std::sync::Mutex::new(ShellPath { found: None, tried: None });
    let inherited = std::env::var("PATH").unwrap_or_default();
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        if let Some(path) = CACHE.lock().unwrap().get(Instant::now(), || login_shell_path(&shell, Duration::from_secs(5))) {
            return format!("{path}:{inherited}");
        }
    }
    inherited
}

/// How long a login shell that didn't answer in time waits before it's asked again, so a Try again a little later works.
const SHELL_PATH_RETRY: Duration = Duration::from_secs(10);

/// The login shell's PATH once found. A shell that was too slow, as one can be right after the Mac starts, is asked
/// again after `SHELL_PATH_RETRY` instead of leaving Wings without Homebrew's programs until it restarts.
struct ShellPath {
    found: Option<String>,
    tried: Option<Instant>,
}

impl ShellPath {
    fn get(&mut self, now: Instant, ask: impl FnOnce() -> Option<String>) -> Option<String> {
        if self.found.is_none() && self.tried.is_none_or(|t| now.duration_since(t) >= SHELL_PATH_RETRY) {
            self.tried = Some(now);
            self.found = ask();
        }
        self.found.clone()
    }
}

/// The PATH a login shell sets up, or `None` when that takes longer than `timeout`. A profile that runs
/// something long would otherwise hold up every program lookup. A job the profile starts in the background
/// can keep the output open after the shell exits, so it's read until the closing marker, not to the end.
#[cfg(unix)]
fn login_shell_path(shell: &str, timeout: Duration) -> Option<String> {
    const MARKER: &str = "__WINGS_PATH__";
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut text, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = stdout.read(&mut chunk) {
            text.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&text);
            let mut parts = text.split(MARKER);
            if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
                let _ = tx.send(path.to_string());
                return;
            }
        }
    });
    let path = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    path.filter(|p| !p.is_empty())
}

pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        return None;
    }
    let names: Vec<String> = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd")] } else { vec![program.into()] };
    std::env::split_paths(&search_path()).flat_map(|dir| names.iter().map(move |n| dir.join(n))).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loading::load, test_support::plugin};
    use std::fs;

    #[cfg(unix)]
    fn fake_shell(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("wings-shell-{name}-{}", std::process::id()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_login_shell_too_slow_at_start_is_asked_again_later() {
        let mut cache = ShellPath { found: None, tried: None };
        let start = Instant::now();
        assert_eq!(cache.get(start, || None), None);
        assert_eq!(cache.get(start + Duration::from_secs(1), || panic!("asked again too soon")), None);
        assert_eq!(cache.get(start + SHELL_PATH_RETRY, || Some("/opt/homebrew/bin".into())).as_deref(), Some("/opt/homebrew/bin"));
        assert_eq!(cache.get(start + SHELL_PATH_RETRY * 4, || panic!("found paths are kept")).as_deref(), Some("/opt/homebrew/bin"));
    }

    #[test]
    #[cfg(unix)]
    fn login_shell_path_gives_up_on_a_slow_profile() {
        let quick = fake_shell("quick", "printf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        assert_eq!(login_shell_path(quick.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        // A job left running in the background keeps the output open after the shell is done.
        let background = fake_shell("background", "sleep 30 &\nprintf '__WINGS_PATH__/fake/bin__WINGS_PATH__'");
        let start = Instant::now();
        assert_eq!(login_shell_path(background.to_str().unwrap(), Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        let hung = fake_shell("hung", "sleep 30");
        let start = Instant::now();
        assert_eq!(login_shell_path(hung.to_str().unwrap(), Duration::from_millis(300)), None);
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        for path in [quick, background, hung] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn exec_rejects_output_over_the_limit_without_waiting_for_the_timeout() {
        let p = plugin(Path::new("/tmp"), &["yes", "echo"], &[]);
        let start = Instant::now();
        // `yes` never stops on its own: the host has to cut it off, long before the 30 s timeout.
        let err = exec(&p, "yes", &[], None, EXEC_TIMEOUT, None).unwrap_err();
        assert_eq!(err, EXEC_OUTPUT_TOO_LARGE);
        assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
        assert!(exec(&p, "echo", &["fine".into()], None, EXEC_TIMEOUT, None).is_ok());
    }

    #[test]
    fn exec_needs_the_declared_subcommand() {
        let p = plugin(Path::new("/tmp"), &["echo hi", "git remote get-url"], &[]);
        assert_eq!(exec(&p, "echo", &["hi".into(), "there".into()], None, EXEC_TIMEOUT, None).unwrap().stdout, "hi there\n");
        assert!(exec(&p, "echo", &["bye".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run echo bye"));
        assert!(exec(&p, "git", &["status".into()], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run git status"));
        let missing = Path::new("/no/such/folder");
        assert!(exec(&p, "echo", &["hi".into()], Some(missing), EXEC_TIMEOUT, None).unwrap_err().contains("is not a folder"));
    }

    #[test]
    fn git_ignores_a_bare_repo_a_plugin_ships() {
        // A source zip can hold a folder shaped like a bare repo; its config would run a command on `git diff`.
        let dir = std::env::temp_dir().join(format!("wings-bare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        let git = |cwd: &Path, args: &[&str]| assert!(Command::new("git").args(args).current_dir(cwd).output().unwrap().status.success(), "git {args:?}");
        let commit = ["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "x"];
        git(&dir.join("src"), &["init", "-q"]);
        for text in ["a", "b"] {
            fs::write(dir.join("src/f"), text).unwrap();
            git(&dir.join("src"), &["add", "f"]);
            git(&dir.join("src"), &commit);
        }
        git(&dir, &["clone", "-q", "--bare", "src", "repo"]);
        let marker = dir.join("ran");
        git(&dir.join("repo"), &["config", "diff.external", &format!("touch {}", marker.display())]);
        let p = plugin(&dir, &["git diff"], &[]);
        let _ = exec(&p, "git", &["diff".into(), "HEAD~1".into(), "HEAD".into()], Some(&dir.join("repo")), EXEC_TIMEOUT, None);
        assert!(!marker.exists(), "git ran the shipped repo's diff.external");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exec_flag_lists_are_checked_on_load() {
        let dir = std::env::temp_dir().join(format!("wings-flags-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |entry: &str| {
            fs::write(
                dir.join("wings-plugin.json"),
                serde_json::json!({ "id": "t", "name": "T", "version": "0", "api": 1, "main": "main.js", "permissions": { "exec": [entry] } }).to_string(),
            )
            .unwrap();
            load(&dir)
        };
        assert!(manifest("az boards work-item update [--id --state]").is_ok());
        assert!(manifest("git branch --show-current").is_ok());
        for bad in ["az x [--id", "az x [id]", "az x [] y", "az x [--id] y", "az x [--a] [--b]"] {
            assert!(manifest(bad).is_err(), "{bad}");
        }
        // `[]` is a command that takes no flags: its values pass, any flag is refused.
        let p = manifest("gh pr diff []").unwrap();
        let diff = |extra: &[&str]| -> Vec<String> { ["pr", "diff", "https://github.com/o/r/pull/1"].iter().chain(extra).map(|a| a.to_string()).collect() };
        assert!(may_exec(&p, "gh", &diff(&[])));
        for extra in [&["--patch"][..], &["--name-only"], &["-R", "o/r"], &["--repo=o/r"], &["--"]] {
            assert!(!may_exec(&p, "gh", &diff(extra)), "{extra:?}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exec_only_runs_declared_programs_without_a_shell() {
        let p = plugin(Path::new("/tmp"), &["echo"], &[]);
        let out = exec(&p, "echo", &["hi; rm -rf /".into()], None, EXEC_TIMEOUT, None).unwrap();
        assert_eq!(out.stdout.trim(), "hi; rm -rf /");
        assert!(exec(&p, "ls", &[], None, EXEC_TIMEOUT, None).unwrap_err().contains("may not run"));
        assert!(exec(&p, "/bin/echo", &[], None, EXEC_TIMEOUT, None).is_err());
    }

    #[test]
    fn exec_streams_lines_from_both_pipes() {
        let p = plugin(Path::new("/tmp"), &["sh"], &[]);
        let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = lines.clone();
        let on_line: OnLine = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        let out = exec(&p, "sh", &["-c".into(), "echo out; echo err >&2".into()], None, EXEC_TIMEOUT, Some(on_line)).unwrap();
        let mut got = lines.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["err", "out"]);
        assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("out\n", "err\n"));
    }

    #[test]
    fn exec_timeout_is_capped() {
        assert_eq!(exec_timeout(None), EXEC_TIMEOUT);
        assert_eq!(exec_timeout(Some(10)), Duration::from_secs(1));
        assert_eq!(exec_timeout(Some(3_600_000)), EXEC_TIMEOUT_MAX);
        let p = plugin(Path::new("/tmp"), &["sleep"], &[]);
        assert!(exec(&p, "sleep", &["5".into()], None, Duration::from_secs(1), None).unwrap_err().contains("timed out"));
    }
}
