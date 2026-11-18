//! What a plugin may start in a new pane, and the folders its panes and commands may run in.

use std::path::{Path, PathBuf};

use crate::{
    cli_rules::{command_refused, is_unsafe_flag},
    manifest::Plugin,
};

/// A word of a pane command. Pane commands are typed into your shell, so words can't hold spaces, quotes,
/// backslashes, `$`, globs or operators; single-quoted, such a word reads the same in sh, bash, zsh and fish.
pub(crate) fn plain_word(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b))
}

const PANE_COMMAND_MAX: usize = 1024;
/// The agent CLIs whose flags a plugin may pass only when its `panes` entry names them.
const AGENT_PROGRAMS: [&str; 2] = ["claude", "opencode"];
/// `openPane` `args` together: room for a prompt that names an issue and says what to do with it.
const PANE_ARGS_MAX: usize = 4096;

/// One `args` value as a single shell word: single-quoted, a `'` inside written as `'\''` and a `\` as `'\\'`. fish reads
/// `\'` and `\\` as escapes even inside single quotes, so a backslash stays outside them to read the same in every shell.
fn shell_quote(arg: &str) -> String {
    format!("'{}'", arg.split('\\').map(|part| part.replace('\'', "'\\''")).collect::<Vec<_>>().join("'\\\\'"))
}

/// The line to type into a new pane's shell for `command`, or `None` for a plain shell. Like `exec`, an entry
/// in `permissions.panes` is a program and the words the command must start with. Words past the entry
/// come from the plugin, so they're single-quoted. `args` go after them as one quoted word each, so a value
/// can hold spaces, like a prompt; one that starts with `-` is refused, since it would read as a flag.
pub fn pane_input(plugin: &Plugin, command: Option<&str>, args: &[String]) -> Result<Option<String>, String> {
    let id = &plugin.manifest.id;
    let Some(declared) = &plugin.manifest.permissions.panes else {
        return Err(format!("{id} may not open panes"));
    };
    let Some(command) = command else {
        return if args.is_empty() { Ok(None) } else { Err("openPane args need a command".into()) };
    };
    // Split on spaces only, so a tab or newline is refused rather than read as a word break.
    let words: Vec<&str> = command.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() || command.len() > PANE_COMMAND_MAX || !words.iter().all(|w| plain_word(w)) {
        return Err(format!("A pane command is up to 1 KB of words using only letters, digits and -_./:=@%+, not {command:?}"));
    }
    let entry = declared
        .iter()
        .map(|e| e.split_ascii_whitespace().collect::<Vec<_>>())
        .filter(|e| words.starts_with(e))
        .max_by_key(Vec::len)
        .ok_or_else(|| format!("{id} may not start {}", words.iter().take(3).copied().collect::<Vec<_>>().join(" ")))?;
    let program = words[0];
    // What gets typed after the program: the command's words, then `args`.
    let typed: Vec<String> = words[1..].iter().map(|w| w.to_string()).chain(args.iter().cloned()).collect();
    if let Some(flag) = typed.iter().find(|w| is_unsafe_flag(program, w)) {
        return Err(format!("{id} may not pass {flag} to {program}"));
    }
    command_refused(id, program, &typed)?;
    // An agent's flags set its permissions, tools and config (claude --dangerously-skip-permissions, --mcp-config,
    // --settings; opencode --auto, --server), so a plugin passes only the ones its entry names. And one value at most,
    // a session id or a prompt: a second word reads as a subcommand (`opencode --session <id> uninstall`).
    if AGENT_PROGRAMS.contains(&program) {
        if let Some(flag) = words[entry.len()..].iter().find(|w| w.starts_with('-')) {
            return Err(format!("{id} may not pass {flag} to {program}"));
        }
        if words.len() - entry.len() + args.len() > 1 {
            return Err(format!("{id} may pass {program} one value after {}, not more", entry.join(" ")));
        }
    }
    if args.iter().map(String::len).sum::<usize>() > PANE_ARGS_MAX {
        return Err(format!("openPane args are up to {} KB together", PANE_ARGS_MAX / 1024));
    }
    // Typed into an interactive shell: a newline would run what follows, and other control characters edit the line.
    if let Some(arg) = args.iter().find(|a| a.is_empty() || a.starts_with('-') || a.chars().any(char::is_control)) {
        return Err(format!("openPane args must be non-empty, not start with -, and have no control characters, not {arg:?}"));
    }
    let extra = words[entry.len()..].iter().map(|w| format!("'{w}'")).chain(args.iter().map(|a| shell_quote(a)));
    Ok(Some(entry.iter().map(|w| w.to_string()).chain(extra).collect::<Vec<_>>().join(" ")))
}

/// The folder a plugin's command runs in. A `cwd` must be inside one of your projects once `..` and symlinks are resolved,
/// like a pane's: in a folder of its own choosing, git compares any two files there and az, gh and glab read its repo. A
/// folder outside them is allowed when it's in a git repo that one of your open panes is in, and with `any_repo` in any
/// git repo (see `worktrees_call_in_any_repo`). Only a repo: in a folder that isn't one, git diff compares any two files.
/// git needs a cwd. Anything else without one runs in `/`, where nothing is a repo, not in the folder Wings itself
/// started in.
pub fn exec_cwd(program: &str, cwd: Option<&str>, projects: &[PathBuf], panes: &[PathBuf], any_repo: bool) -> Result<PathBuf, String> {
    let Some(cwd) = cwd else {
        return if program == "git" { Err("git runs in a folder inside one of your Wings projects: pass its cwd".into()) } else { Ok(PathBuf::from("/")) };
    };
    pane_cwd(Path::new(cwd), projects, None).map(|(_, dir)| dir).or_else(|refused| {
        let dir = Path::new(cwd).canonicalize().ok().filter(|d| d.is_dir()).ok_or_else(|| refused.clone())?;
        let repo = repo_root(&dir).ok_or_else(|| refused.clone())?;
        let open = || panes.iter().filter_map(|p| p.canonicalize().ok()).any(|p| repo_root(&p).as_ref() == Some(&repo));
        if any_repo || open() {
            Ok(dir)
        } else {
            Err(format!("{cwd} isn't inside any of your Wings projects or a repo open in a pane"))
        }
    })
}

/// Whether a built-in plugin may run this in any git repo on the Mac: the exact calls Worktrees makes for its "Elsewhere"
/// repos, which a repo found on disk may make anyone's. The reads run no program a repo names (measured: `worktree list`,
/// `rev-parse`, `branch --list`, `worktree prune`). Removing runs the repo's own settings and hooks (`worktree remove`
/// its file system monitor and clean filters, `branch -d` its reference-transaction hook), so the plugin warns before
/// it and has no bulk remove there. Never a force flag, and never `status`, `log` or `diff`.
pub fn worktrees_call_in_any_repo(builtin: bool, program: &str, args: &[String]) -> bool {
    // An operand that starts with `-` would be read as an option.
    let operand = |a: &str| !a.is_empty() && !a.starts_with('-') && !a.chars().any(char::is_control);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    builtin
        && program == "git"
        && match args.as_slice() {
            ["worktree", "list", "--porcelain"] | ["worktree", "prune", "--verbose"] => true,
            ["rev-parse", "--show-toplevel"] | ["rev-parse", "--abbrev-ref", "origin/HEAD"] => true,
            ["rev-parse", "--verify", "--quiet", rev] => rev.strip_suffix("^{commit}").is_some_and(operand),
            ["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"] => true,
            ["branch", "--list", merged, "--format=%(refname:short)"] => merged.strip_prefix("--merged=").is_some_and(operand),
            ["worktree", "remove", "--", path] => operand(path),
            ["branch", "-d", branch] => operand(branch),
            _ => false,
        }
}

/// The nearest folder at or above `dir` that holds a `.git`, a folder or a worktree's file.
fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// The folder a plugin's pane may start in: inside one of your projects once `..` and symlinks are resolved,
/// so neither can lead out of it. Returns which project it belongs to and the resolved folder. The project
/// on screen (`current`) wins when it holds the folder, so a split stays in its tab's project; otherwise the
/// deepest project that holds it.
pub fn pane_cwd(cwd: &Path, projects: &[PathBuf], current: Option<usize>) -> Result<(usize, PathBuf), String> {
    let dir = cwd.canonicalize().ok().filter(|d| d.is_dir()).ok_or_else(|| format!("{} is not a folder", cwd.display()))?;
    let roots: Vec<Option<PathBuf>> = projects.iter().map(|p| p.canonicalize().ok()).collect();
    let holds = |i: &usize| roots[*i].as_ref().is_some_and(|root| dir.starts_with(root));
    let depth = |i: &usize| roots[*i].as_ref().map_or(0, |root| root.components().count());
    let project = current.filter(holds).or_else(|| (0..roots.len()).filter(holds).max_by_key(depth));
    project.map(|i| (i, dir)).ok_or_else(|| format!("{} isn't inside any of your Wings projects", cwd.display()))
}

/// Takes the grant `open_pane` left for a tab in `space_id` starting in `dir`, a folder outside that project, so
/// `pane_create` starts it there. Plugin panes never get one.
pub fn take_grant(grants: &mut Vec<(String, PathBuf)>, space_id: &str, dir: &Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().ok()?;
    let at = grants.iter().position(|(s, d)| s == space_id && *d == dir)?;
    Some(grants.remove(at).1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loading::load, permissions::may_use_panes, test_support::with_panes};
    use std::{fs, process::Command};

    #[test]
    fn pane_commands_must_be_declared() {
        let p = with_panes(Some(&["lazygit", "npm run dev", "claude --resume", "git log"]));
        let input = |command: Option<&str>| pane_input(&p, command, &[]);
        assert_eq!(input(None).unwrap(), None);
        assert_eq!(input(Some("lazygit")).unwrap().as_deref(), Some("lazygit"));
        assert_eq!(input(Some("  npm   run dev ")).unwrap().as_deref(), Some("npm run dev"));
        // Words the plugin adds past the declared entry are quoted.
        assert_eq!(input(Some("npm run dev --port=3000")).unwrap().as_deref(), Some("npm run dev '--port=3000'"));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        assert_eq!(input(Some(&format!("claude --resume {id}"))).unwrap(), Some(format!("claude --resume '{id}'")));

        for undeclared in ["rm -rf /", "npm run build", "npm", "claude", "lazygitx", "/usr/bin/lazygit"] {
            assert!(input(Some(undeclared)).unwrap_err().contains("may not start"), "{undeclared}");
        }
        // Nothing a shell would read as more than plain words: operators, quotes, expansions, globs, newlines.
        for smuggled in [
            "lazygit; rm -rf ~",
            "lazygit && curl x",
            "lazygit | sh",
            "lazygit $(id)",
            "lazygit `id`",
            "lazygit 'a'",
            "lazygit a\\",
            "lazygit ~",
            "lazygit *",
            "lazygit >x",
            "lazygit\nrm",
            "lazygit\r",
            "npm run dev #",
            "",
        ] {
            assert!(input(Some(smuggled)).unwrap_err().contains("only letters"), "{smuggled:?}");
        }
        assert!(input(Some(&format!("lazygit {}", "a".repeat(1100)))).is_err());
        // The same flags `exec` refuses.
        assert!(input(Some("git log -ccore.pager=sh")).unwrap_err().contains("may not pass"));
        // Claude takes only the flags the entry names; a prompt word is fine.
        let claude = with_panes(Some(&["claude", "claude --resume"]));
        assert_eq!(pane_input(&claude, Some("claude Continue-issue-A-1"), &[]).unwrap().as_deref(), Some("claude 'Continue-issue-A-1'"));
        assert_eq!(pane_input(&claude, Some(&format!("claude --resume {id}")), &[]).unwrap(), Some(format!("claude --resume '{id}'")));
        // `args` keep their spaces as one quoted word, a quote inside escaped; flags and control characters are refused.
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pane_input(&claude, Some("claude"), &args(&["Continue issue A-1. It's in review"])).unwrap().as_deref(),
            Some("claude 'Continue issue A-1. It'\\''s in review'")
        );
        for bad in [&["--dangerously-skip-permissions"][..], &["line\nrm -rf ~"], &["tab\there"], &[""]] {
            assert!(pane_input(&claude, Some("claude"), &args(bad)).is_err(), "{bad:?}");
        }
        assert!(pane_input(&claude, None, &args(&["x"])).is_err());
        assert!(pane_input(&claude, Some("claude"), &args(&[&"a".repeat(5000)])).is_err());
        for flag in [
            "claude --dangerously-skip-permissions",
            "claude --permission-mode=bypassPermissions",
            "claude x --mcp-config=a.json",
            "claude --resume x --settings=s.json",
            "claude -p x",
        ] {
            assert!(pane_input(&claude, Some(flag), &[]).unwrap_err().contains("may not pass"), "{flag}");
        }
        // opencode the same: its prompt and session, but not --auto (approves every permission) or --server.
        let opencode = with_panes(Some(&["opencode --prompt", "opencode --session"]));
        assert_eq!(pane_input(&opencode, Some("opencode --session ses_abc12345"), &[]).unwrap().as_deref(), Some("opencode --session 'ses_abc12345'"));
        assert_eq!(
            pane_input(&opencode, Some("opencode --prompt"), &args(&["Continue issue A-1."])).unwrap().as_deref(),
            Some("opencode --prompt 'Continue issue A-1.'")
        );
        for flag in ["opencode --prompt --auto", "opencode --session ses_abc12345 --server=http://x", "opencode --session x -c"] {
            assert!(pane_input(&opencode, Some(flag), &[]).unwrap_err().contains("may not pass"), "{flag}");
        }
        // A second value would be read as a subcommand, so an agent gets one.
        assert!(pane_input(&opencode, Some("opencode --session ses_abc12345 uninstall"), &[]).unwrap_err().contains("one value"));
        assert!(pane_input(&opencode, Some("opencode --session ses_abc12345"), &args(&["uninstall"])).unwrap_err().contains("one value"));
        assert!(pane_input(&opencode, Some("opencode --prompt"), &args(&["hi", "uninstall"])).unwrap_err().contains("one value"));
        assert!(pane_input(&claude, Some("claude mcp remove"), &[]).unwrap_err().contains("one value"));

        // `[]` allows plain shells only; no `panes` allows nothing, not even focus.
        let shells = with_panes(Some(&[]));
        assert_eq!(pane_input(&shells, None, &[]).unwrap(), None);
        assert!(pane_input(&shells, Some("lazygit"), &[]).unwrap_err().contains("may not start"));
        assert!(may_use_panes(&shells));
        let none = with_panes(None);
        assert!(pane_input(&none, None, &[]).unwrap_err().contains("may not open panes"));
        assert!(!may_use_panes(&none));
    }

    #[test]
    fn a_pane_value_with_a_backslash_stays_one_word_in_fish() {
        // In fish `\'` is a quote even inside single quotes, so this value would close its word and run `touch`.
        let value = "\\'; touch /tmp/wings-pwned; # C:\\dir\\";
        let quoted = shell_quote(value);
        let (mut chars, mut inside) = (quoted.chars(), false);
        while let Some(c) = chars.next() {
            match (inside, c) {
                (_, '\'') => inside = !inside,
                (false, '\\') => _ = chars.next(),
                (true, '\\') => panic!("a backslash inside single quotes, which fish reads as an escape: {quoted}"),
                _ => {}
            }
        }
        for shell in ["sh", "bash"] {
            let out = Command::new(shell).args(["-c", &format!("printf %s {quoted}")]).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), value, "{shell}");
        }
    }

    #[test]
    fn pane_entries_are_checked_when_the_plugin_loads() {
        let dir = std::env::temp_dir().join(format!("wings-panes-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |panes: &[&str]| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": { "panes": panes } });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir)
        };
        assert_eq!(manifest(&["lazygit", "npm run dev"]).unwrap().manifest.permissions.panes.unwrap(), ["lazygit", "npm run dev"]);
        for bad in ["npm run dev && curl x | sh", "", " ", "echo $HOME"] {
            assert!(manifest(&[bad]).err().unwrap().contains("panes entry"), "{bad:?}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grant_is_for_one_tab_in_one_project_and_folder() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let mut grants = vec![("s1".to_string(), dir.clone())];
        assert_eq!(take_grant(&mut grants, "s2", &dir), None);
        assert_eq!(take_grant(&mut grants, "s1", Path::new("/")), None);
        assert_eq!(take_grant(&mut grants, "s1", &dir), Some(dir.clone()));
        assert_eq!(take_grant(&mut grants, "s1", &dir), None, "used up");
    }

    #[test]
    fn exec_runs_inside_a_project() {
        let base = std::env::temp_dir().join(format!("wings-exec-cwd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("proj/src")).unwrap();
        fs::create_dir_all(base.join("other")).unwrap();
        std::os::unix::fs::symlink(base.join("other"), base.join("proj/out")).unwrap();
        let projects = [base.join("proj")];
        let at = |program: &str, cwd: Option<&Path>| exec_cwd(program, cwd.map(|c| c.to_str().unwrap()), &projects, &[], false);
        assert_eq!(at("git", Some(&base.join("proj/src"))).unwrap(), base.join("proj/src").canonicalize().unwrap());
        // A folder outside every project, or one a `..` or a symlink leads out to, is refused.
        for cwd in [base.join("other"), base.join("proj/src/../../other"), base.join("proj/out"), PathBuf::from("/")] {
            assert!(at("gh", Some(&cwd)).unwrap_err().contains("isn't inside"), "{}", cwd.display());
        }
        assert!(at("git", None).is_err());
        assert_eq!(at("az", None).unwrap(), PathBuf::from("/"));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn exec_runs_in_a_repo_a_pane_is_in_and_built_ins_in_any_repo() {
        let base = std::env::temp_dir().join(format!("wings-exec-repo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for dir in ["proj", "repo/.git", "repo/src", "other/.git", "plain/sub"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let projects = [base.join("proj")];
        let panes = [base.join("repo/src"), base.join("plain")];
        let at = |cwd: &str, builtin| exec_cwd("git", Some(base.join(cwd).to_str().unwrap()), &projects, &panes, builtin);
        // The repo a pane is in, at its root or below, for any plugin.
        assert_eq!(at("repo", false).unwrap(), base.join("repo").canonicalize().unwrap());
        assert!(at("repo/src", false).is_ok());
        // Another repo only for the calls Worktrees makes, from a built-in plugin; a pane's folder that isn't a repo never, since git
        // diff there reads any file.
        assert!(at("other", false).unwrap_err().contains("open in a pane"));
        assert!(at("other", true).is_ok());
        for cwd in ["plain", "plain/sub"] {
            assert!(at(cwd, false).is_err() && at(cwd, true).is_err(), "{cwd}");
        }
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let allowed = |builtin, a: &[&str]| worktrees_call_in_any_repo(builtin, "git", &args(a));
        // Each call Worktrees makes passes for a built-in plugin and for no other.
        for a in [
            &["worktree", "list", "--porcelain"][..],
            &["worktree", "prune", "--verbose"],
            &["rev-parse", "--show-toplevel"],
            &["rev-parse", "--abbrev-ref", "origin/HEAD"],
            &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"],
            &["branch", "--list", "--merged=origin/main", "--format=%(refname:short)"],
            &["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"],
            &["worktree", "remove", "--", "/Users/me/dev/repo-fix"],
            &["branch", "-d", "feature/x"],
        ] {
            assert!(allowed(true, a) && !allowed(false, a), "{a:?}");
        }
        // Force, `-D`, config and location flags, extra arguments, operands that look like options, and other calls.
        for a in [
            &["worktree", "remove", "--force", "--", "/p"][..],
            &["worktree", "remove", "-f", "--", "/p"],
            &["worktree", "remove", "/p"],
            &["worktree", "remove", "--", "-f"],
            &["worktree", "remove", "--", "/p", "/q"],
            &["worktree", "remove", "--", ""],
            &["branch", "-D", "x"],
            &["branch", "-d", "-D"],
            &["branch", "-d", "--force", "x"],
            &["branch", "-d", "-f", "x"],
            &["branch", "-d", "x", "y"],
            &["branch", "--delete", "x"],
            &["-c", "core.fsmonitor=touch x", "worktree", "list", "--porcelain"],
            &["-C", "/elsewhere", "worktree", "list", "--porcelain"],
            &["--git-dir=/x", "worktree", "list", "--porcelain"],
            &["worktree", "list", "--porcelain", "-c", "x=y"],
            &["worktree", "list"],
            &["worktree", "list", "--porcelain", "--extra"],
            &["worktree", "prune"],
            &["worktree", "prune", "--verbose", "--expire=now"],
            &["worktree", "add", "/p"],
            &["worktree", "lock", "/p"],
            &["rev-parse", "--verify", "--quiet", "--help^{commit}"],
            &["rev-parse", "--verify", "--quiet", "origin/main"],
            &["rev-parse", "--verify", "--quiet", "origin/main^{commit}", "--extra"],
            &["rev-parse", "--git-dir"],
            &["rev-parse", "--show-toplevel", "--extra"],
            &["branch", "--list", "--merged=-x", "--format=%(refname:short)"],
            &["branch", "--list", "--merged=main", "--format=%(refname)"],
            &["branch", "--list", "--format=%(refname:short)", "--merged=main"],
            &["branch", "--list", "--contains=x", "--format=%(refname:short)"],
            &["branch", "--list"],
            &["status"],
            &["status", "--porcelain"],
            &["log", "--max-count=1", "--format=%ct"],
            &["diff"],
            &[],
        ] {
            assert!(!allowed(true, a), "{a:?}");
        }
        assert!(!worktrees_call_in_any_repo(true, "sh", &args(&["worktree", "list", "--porcelain"])));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn pane_cwd_must_be_inside_a_project() {
        // Canonical, since macOS temp folders sit behind the /var symlink.
        let root = std::env::temp_dir().join(format!("wings-cwd-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        for dir in ["app/src", "app/nested/deep", "app2", "outside"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("app/file.txt"), "").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("app/link-out")).unwrap();
        let projects = [root.join("app"), root.join("app/nested"), root.join("app2")];
        let cwd = |dir: &str, current| pane_cwd(&root.join(dir), &projects, current);

        assert_eq!(cwd("app", None).unwrap(), (0, root.join("app")));
        assert_eq!(cwd("app/src", None).unwrap(), (0, root.join("app/src")));
        // The deepest project holds it, unless the project on screen does.
        assert_eq!(cwd("app/nested/deep", None).unwrap().0, 1);
        assert_eq!(cwd("app/nested/deep", Some(0)).unwrap().0, 0);
        assert_eq!(cwd("app/src", Some(2)).unwrap().0, 0);

        for outside in ["outside", "app/../outside", "app/link-out", "app/src/../../outside", "."] {
            assert!(cwd(outside, Some(0)).unwrap_err().contains("isn't inside"), "{outside}");
        }
        // `app2` only shares a prefix with `app`; with app2 gone it's in no project.
        assert!(pane_cwd(&root.join("app2"), &projects[..2], None).unwrap_err().contains("isn't inside"));
        assert!(pane_cwd(Path::new("/"), &projects, None).unwrap_err().contains("isn't inside"));
        assert!(cwd("app/missing", None).unwrap_err().contains("is not a folder"));
        assert!(cwd("app/file.txt", None).unwrap_err().contains("is not a folder"));
        let _ = fs::remove_dir_all(&root);
    }
}
