use super::*;
use std::os::unix::ffi::OsStrExt;

/// A folder under `/tmp` for one test, short enough for a socket path, removed when it goes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = PathBuf::from(format!("/tmp/wl-{name}-{}", &random_hex()[..8]));
        std::fs::create_dir(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_job_runs_once_leaves_its_server_alone_and_starts_with_the_sentinel() {
    let mut cmd = Command::new("/a<&>/wings-terminal-server");
    cmd.args(["serve", "--socket", "/data/v4.sock"]).env("LESS_TERMCAP_md", "\x1b[1m\r").env_remove("HOME");
    let job = plist(&cmd, Path::new("/data/v4.log"), "dev.wings.native.terminal-server.1.2", Path::new("/tmp/x/ready"), 0o027).unwrap();
    assert!(job.contains(
        "<array><string>/a&lt;&amp;&gt;/wings-terminal-server</string><string>--wings-launchd</string>\
         <string>serve</string><string>--socket</string><string>/data/v4.sock</string></array>"
    ));
    assert!(!job.contains("LESS_TERMCAP_md"), "the environment goes over the socket");
    assert!(!job.contains("<key>HOME</key>"));
    assert!(job.contains(&format!("<key>{DETACH}</key><string>/tmp/x/ready</string>")));
    assert!(job.contains("<key>RunAtLoad</key><true/>"));
    assert!(job.contains("<key>KeepAlive</key><false/>"));
    assert!(job.contains("<key>AbandonProcessGroup</key><true/>"));
    assert!(job.contains("<key>Umask</key><integer>23</integer>"));
    assert!(job.contains("<key>StandardErrorPath</key><string>/data/v4.log</string>"));
    assert!(!job.contains("AssociatedBundleIdentifiers"));

    let scratch = Scratch::new("plist");
    let file = scratch.0.join("job.plist");
    std::fs::write(&file, &job).unwrap();
    let lint = Command::new("/usr/bin/plutil").arg("-lint").arg(&file).output().unwrap();
    assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
    let json = Command::new("/usr/bin/plutil").args(["-convert", "json", "-o", "-"]).arg(&file).output().unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(parsed["Label"], "dev.wings.native.terminal-server.1.2");
    assert_eq!(parsed["KeepAlive"], false);
    assert_eq!(parsed["AbandonProcessGroup"], true);
    assert_eq!(parsed["ProgramArguments"][0], "/a<&>/wings-terminal-server");
    assert_eq!(parsed["ProgramArguments"][1], SENTINEL);
}

#[test]
fn environment_transfer_keeps_any_unix_bytes_removals_and_every_limit() {
    let mut cmd = Command::new("wings-terminal-server");
    cmd.env("LESS_TERMCAP_md", OsStr::from_bytes(b"\x1b[1m\r\xff")).env_remove("HOME").env(DETACH, "old");
    let captured = Inherited::capture(&cmd).unwrap();
    let received: Inherited = serde_json::from_slice(&serde_json::to_vec(&captured).unwrap()).unwrap();
    assert_eq!(received.limits, captured.limits);
    assert!(received.limits.len() > libc::RLIMIT_NOFILE as usize);
    let env: BTreeMap<_, _> = received.env.into_iter().collect();
    assert_eq!(env.get(b"LESS_TERMCAP_md".as_slice()), Some(&b"\x1b[1m\r\xff".to_vec()));
    for name in [b"HOME".as_slice(), DETACH.as_bytes()] {
        assert!(!env.contains_key(name));
    }
    assert_eq!(env.get(b"XPC_SERVICE_NAME".as_slice()), Some(&b"0".to_vec()));
}

#[test]
fn only_old_finished_jobs_of_ours_are_stale() {
    let now = Duration::from_secs(1000);
    let old = label(1, Duration::from_secs(900));
    let list = format!(
        "PID\tStatus\tLabel\n-\t0\t{old}\n42\t0\t{}\n-\t0\t{}\n-\t0\tother.4.{}\n-\t0\t{PREFIX}v3\n-\t78\t{PREFIX}x.{}\n",
        label(2, Duration::from_secs(900)),
        label(3, Duration::from_secs(990)),
        Duration::from_secs(900).as_nanos(),
        Duration::from_secs(900).as_nanos(),
    );
    assert_eq!(stale_labels(&list, now), vec![old.as_str()]);
}

#[test]
fn stale_folders_need_a_dead_starter_and_never_follow_symlinks() {
    let scratch = Scratch::new("stale");
    let mut child = Command::new("/usr/bin/true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    let now = Duration::from_secs(1000);
    let make = |pid: u32, stamp: u64| {
        let path = scratch.0.join(format!("{DIR_PREFIX}{pid}-{}-{}", Duration::from_secs(stamp).as_nanos(), random_hex()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("job.plist"), "interrupted before unlink").unwrap();
        UnixListener::bind(path.join("ready")).unwrap();
        path
    };
    let abandoned = make(dead, 900);
    let recent = make(dead, 990);
    let live = make(std::process::id(), 900);
    let kept = scratch.0.join("kept");
    std::fs::create_dir(&kept).unwrap();
    let link = scratch.0.join(format!("{DIR_PREFIX}{dead}-{}-{}", Duration::from_secs(900).as_nanos(), random_hex()));
    std::os::unix::fs::symlink(&kept, &link).unwrap();
    remove_stale_dirs(&scratch.0, now);
    assert!(!abandoned.exists(), "a dead starter's socket and plist are removed");
    assert!(recent.exists(), "recent starts are left alone");
    assert!(live.exists(), "live starters are left alone");
    assert!(link.exists() && kept.exists(), "symlinks are left alone");
}

#[test]
fn the_handshake_sends_the_environment_and_waits_for_the_ack() {
    let scratch = Scratch::new("hand");
    let log = scratch.0.join("v.log");
    let mut cmd = Command::new("/bin/server");
    cmd.env("WINGS_HANDSHAKE_PROBE", "yes");
    let (got, receiver) = std::sync::mpsc::channel();
    launch(&cmd, &log, &scratch.0, |job, file| {
        assert!(std::fs::read_to_string(file).unwrap().contains("<key>KeepAlive</key><false/>"));
        assert_eq!(std::fs::metadata(&job.dir).map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777).unwrap(), 0o700);
        let socket = job.socket.clone();
        // What `receive` does in the detached server, short of replacing this test's environment.
        std::thread::spawn(move || {
            let mut ready = UnixStream::connect(socket).unwrap();
            let inherited: Inherited = serde_json::from_reader(&mut ready).unwrap();
            ready.write_all(&[1]).unwrap();
            got.send(inherited).unwrap();
        });
        Ok(())
    })
    .unwrap();
    let inherited = receiver.recv().unwrap();
    assert!(inherited.env.contains(&(b"WINGS_HANDSHAKE_PROBE".to_vec(), b"yes".to_vec())));
    assert!(log.exists(), "the log is made before launchd opens it");
    let left: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().flatten().map(|e| e.file_name()).filter(|n| n != "v.log").collect();
    assert!(left.is_empty(), "the start's folder is removed: {left:?}");
}

#[test]
fn a_failed_start_returns_its_error_and_removes_its_folder() {
    let scratch = Scratch::new("fail");
    let error = launch(&Command::new("/bin/server"), &scratch.0.join("v.log"), &scratch.0, |_, _| Err(io::Error::other("bootstrap refused"))).unwrap_err();
    assert_eq!(error.to_string(), "bootstrap refused");
    let left: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().flatten().map(|e| e.file_name()).filter(|n| n != "v.log").collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn unrepresentable_values_fail_the_plist_so_the_app_starts_directly() {
    for value in [OsStr::from_bytes(b"bad\xff"), OsStr::new("line\rbreak"), OsStr::new("\x1b[0m")] {
        assert!(xml(value).is_err());
    }
    assert_eq!(xml(OsStr::new("<&>\"'\t\n")).unwrap(), "&lt;&amp;&gt;&quot;'\t\n");
}
