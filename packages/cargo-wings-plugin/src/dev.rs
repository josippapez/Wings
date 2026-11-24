//! `cargo wings-plugin dev`: builds the plugin when its files change, then has the running Wings read the folder again
//! (`wings plugin link <folder>`), which restarts the plugin's frames. A failed build prints its error and keeps watching.
//!
//! The link step runs the `wings` command rather than speaking Wings' socket protocol itself: `wings plugin link` is the
//! same request, and the command stays the one place that knows where the socket is and what Wings answers.

use crate::frames::{Runner, build_rust};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant, SystemTime},
};

/// Where the app has its `wings` command, when it isn't on the PATH.
pub const APP_WINGS: &str = "/Applications/Wings.app/Contents/MacOS/wings";

/// The build writes `dist/`, and these hold files nobody edits by hand. `target` is cargo's, in `rust/`.
const IGNORED: [&str; 4] = ["dist", "node_modules", ".git", "target"];

const POLL: Duration = Duration::from_millis(300);
const DEBOUNCE: Duration = Duration::from_millis(150);
const RETRY: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq)]
pub enum LinkError {
    /// No `wings` command on the PATH or in the app.
    NoCommand,
    NotRunning,
    /// An older Wings that doesn't know the link request.
    TooOld,
    Failed(String),
}

/// The `wings` command: on the PATH, or inside the app.
pub fn find_wings(path_var: Option<std::ffi::OsString>, app: &Path) -> Option<PathBuf> {
    let on_path = path_var.iter().flat_map(env::split_paths).map(|dir| dir.join("wings")).find(|file| file.is_file());
    on_path.or_else(|| app.is_file().then(|| app.to_path_buf()))
}

/// Has Wings read this plugin folder again and restart its plugin. Returns what the command printed.
pub fn link(runner: &dyn Runner, wings: Option<&Path>, dir: &Path) -> Result<String, LinkError> {
    let wings = wings.ok_or(LinkError::NoCommand)?;
    let args = ["plugin".to_string(), "link".to_string(), dir.to_string_lossy().into_owned()];
    let out = runner.run(&wings.to_string_lossy(), &args, dir, true);
    if out.status == Some(0) {
        return Ok(out.stdout);
    }
    let message = out.stderr.trim().trim_start_matches("wings: ").to_string();
    Err(if message.contains("isn't running") {
        LinkError::NotRunning
    } else if message.contains("unknown op") {
        LinkError::TooOld
    } else {
        LinkError::Failed(if message.is_empty() { "the wings command failed".into() } else { message })
    })
}

/// Says what a link did. True when it is worth trying again: Wings wasn't open.
pub fn report_link(result: Result<String, LinkError>, log: &mut dyn FnMut(&str), error: &mut dyn FnMut(&str)) -> bool {
    match result {
        Ok(out) if out.contains("waiting for approval") => {
            log("Linked. Wings is waiting for you to approve the plugin: open the Plugins sheet and review its access.");
            false
        }
        Ok(_) => {
            log("Linked: Wings restarted the plugin.");
            false
        }
        Err(LinkError::NotRunning) => {
            error(&format!("Wings isn't running. Open Wings and this will link the plugin, trying again every {} s.", RETRY.as_secs()));
            true
        }
        Err(LinkError::TooOld) => {
            error("This Wings is too old to link a plugin folder. Update Wings, then save a file here to try again.");
            false
        }
        Err(LinkError::NoCommand) => {
            error(&format!("Couldn't find the wings command. Install Wings, or put its command on your PATH: {APP_WINGS}"));
            false
        }
        Err(LinkError::Failed(message)) => {
            error(&format!("Wings didn't take the plugin: {message}"));
            false
        }
    }
}

/// The files that can change the build, with when and how big, so a save shows as a difference between two of these.
pub fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (Option<SystemTime>, u64)> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (Option<SystemTime>, u64)>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if IGNORED.contains(&entry.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            match fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => walk(&path, out),
                Ok(meta) => {
                    out.insert(path, (meta.modified().ok(), meta.len()));
                }
                Err(_) => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}

/// One build, then one link. Returns whether the link is worth trying again. A failed build prints its error.
pub fn cycle(
    build: &mut dyn FnMut() -> Result<usize, String>,
    link: &mut dyn FnMut() -> Result<String, LinkError>,
    log: &mut dyn FnMut(&str),
    error: &mut dyn FnMut(&str),
) -> bool {
    match build() {
        Ok(n) => {
            log(&format!("Built {n} file{}.", if n == 1 { "" } else { "s" }));
            report_link(link(), log, error)
        }
        Err(message) => {
            error(&message);
            false
        }
    }
}

/// Watches `dir` until the process is stopped. Ctrl+C leaves the plugin linked in Wings.
pub fn watch(dir: &Path, runner: &dyn Runner) -> ! {
    let wings = find_wings(env::var_os("PATH"), Path::new(APP_WINGS));
    let mut log = |s: &str| println!("{s}");
    let mut error = |s: &str| eprintln!("{s}");
    let mut build = || build_rust(dir, false, runner).map(|b| b.built.len());
    let mut link_now = || link(runner, wings.as_deref(), dir);
    println!("Watching {}", dir.display());
    let mut seen = snapshot(dir);
    let mut retry = cycle(&mut build, &mut link_now, &mut log, &mut error);
    let mut tried = Instant::now();
    loop {
        sleep(POLL);
        if snapshot(dir) != seen {
            sleep(DEBOUNCE);
            // A save during the build builds again.
            loop {
                seen = snapshot(dir);
                retry = cycle(&mut build, &mut link_now, &mut log, &mut error);
                tried = Instant::now();
                if snapshot(dir) == seen {
                    break;
                }
            }
        } else if retry && tried.elapsed() >= RETRY {
            retry = report_link(link_now(), &mut log, &mut error);
            tried = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{Output, tests::Scratch};
    use std::cell::RefCell;

    struct Wings(RefCell<Vec<Vec<String>>>, Output);
    impl Runner for Wings {
        fn run(&self, program: &str, args: &[String], _cwd: &Path, capture: bool) -> Output {
            assert!(capture);
            self.0.borrow_mut().push([program.to_string()].into_iter().chain(args.iter().cloned()).collect());
            Output { status: self.1.status, stdout: self.1.stdout.clone(), stderr: self.1.stderr.clone() }
        }
    }
    fn wings(status: i32, stdout: &str, stderr: &str) -> Wings {
        Wings(RefCell::new(Vec::new()), Output { status: Some(status), stdout: stdout.into(), stderr: stderr.into() })
    }

    #[test]
    fn link_runs_wings_plugin_link_on_the_folder() {
        let w = wings(0, "Demo 0.1.0 is on.\n", "");
        assert_eq!(link(&w, Some(Path::new("/bin/wings")), Path::new("/p/demo")), Ok("Demo 0.1.0 is on.\n".into()));
        assert_eq!(w.0.borrow()[0], ["/bin/wings", "plugin", "link", "/p/demo"]);
    }

    #[test]
    fn link_tells_wings_closed_an_old_wings_and_a_missing_command_apart() {
        let dir = Path::new("/p");
        let wings_at = Some(Path::new("/bin/wings"));
        assert_eq!(link(&wings(1, "", "wings: Wings isn't running. Open Wings and try again.\n"), wings_at, dir), Err(LinkError::NotRunning));
        assert_eq!(link(&wings(1, "", "wings: unknown op\n"), wings_at, dir), Err(LinkError::TooOld));
        assert_eq!(link(&wings(1, "", "wings: it broke\n"), wings_at, dir), Err(LinkError::Failed("it broke".into())));
        assert_eq!(link(&wings(0, "", ""), None, dir), Err(LinkError::NoCommand));
    }

    #[test]
    fn a_link_says_what_happened_and_only_a_closed_wings_is_tried_again() {
        let say = |r| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let retry = report_link(r, &mut |s| out.push(s.to_string()), &mut |s| err.push(s.to_string()));
            (retry, out, err)
        };
        assert_eq!(say(Ok("Demo 0.1.0 is on.".into())), (false, vec!["Linked: Wings restarted the plugin.".to_string()], vec![]));
        let (retry, out, _) = say(Ok("Demo 0.1.0 is waiting for approval.".into()));
        assert!(!retry && out[0].contains("approve the plugin"));
        let (retry, _, err) = say(Err(LinkError::NotRunning));
        assert!(retry && err[0].contains("Wings isn't running") && err[0].contains("every 2 s"));
        assert!(!say(Err(LinkError::TooOld)).0 && !say(Err(LinkError::Failed("x".into()))).0 && !say(Err(LinkError::NoCommand)).0);
    }

    #[test]
    fn a_failed_build_prints_its_error_and_doesnt_link() {
        let (mut out, mut err, mut linked) = (Vec::new(), Vec::new(), 0);
        let retry = cycle(&mut || Err("cargo build failed".into()), &mut || { linked += 1; Ok(String::new()) }, &mut |s| out.push(s.to_string()), &mut |s| err.push(s.to_string()));
        assert!(!retry && linked == 0 && out.is_empty() && err == ["cargo build failed"]);
        let retry = cycle(&mut || Ok(2), &mut || { linked += 1; Ok(String::new()) }, &mut |s| out.push(s.to_string()), &mut |s| err.push(s.to_string()));
        assert!(!retry && linked == 1);
        assert_eq!(out[0], "Built 2 files.");
    }

    #[test]
    fn a_save_changes_the_snapshot_and_the_build_output_does_not() {
        let root = Scratch::new("dev");
        let dir = &root.0;
        fs::create_dir_all(dir.join("rust/src")).unwrap();
        fs::create_dir_all(dir.join("rust/target")).unwrap();
        fs::create_dir_all(dir.join("dist/native")).unwrap();
        fs::write(dir.join("rust/src/lib.rs"), "a").unwrap();
        let before = snapshot(dir);
        assert_eq!(before.len(), 1);
        fs::write(dir.join("rust/target/junk"), "x").unwrap();
        fs::write(dir.join("dist/native/main.wasm"), "x").unwrap();
        assert_eq!(snapshot(dir), before);
        fs::write(dir.join("rust/src/lib.rs"), "ab").unwrap();
        assert_ne!(snapshot(dir), before);
        fs::write(dir.join("wings-plugin.json"), "{}").unwrap();
        assert_eq!(snapshot(dir).len(), 2);
    }

    #[test]
    fn wings_is_found_on_the_path_or_in_the_app() {
        let root = Scratch::new("find");
        let bin = root.0.join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("wings"), "").unwrap();
        let app = root.0.join("app-wings");
        assert_eq!(find_wings(Some(bin.clone().into_os_string()), &app), Some(bin.join("wings")));
        assert_eq!(find_wings(None, &app), None);
        fs::write(&app, "").unwrap();
        assert_eq!(find_wings(Some("/nonexistent".into()), &app), Some(app));
    }
}
