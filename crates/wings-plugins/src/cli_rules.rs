//! What no manifest can allow in a call to git, gh, glab or az: unsafe flags, settings commands, tokens sent elsewhere.

use std::path::Path;

use serde_json::Value;

/// The resource `az rest` gets a token for when it calls Azure DevOps.
pub(crate) const AZURE_DEVOPS_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";

/// Flags that turn an allowed command into running other commands, writing or reading files outside the
/// repo, changing data on the server, or printing a token. Refused whatever the manifest says.
pub(crate) fn is_unsafe_flag(program: &str, arg: &str) -> bool {
    // `abbrev`: git and az accept any unambiguous prefix of a long option, so `--upload=` means
    // `--upload-pack=`. gh and glab only take exact names.
    let (short, long, abbrev): (&[char], &[&str], bool) = match program {
        // Config overrides can point core.fsmonitor, core.sshCommand or an alias at any command.
        "git" => (&['c'], &["--config-env", "--exec-path", "--upload-pack", "--receive-pack", "--output", "--ext-diff", "--textconv", "--no-index"], true),
        // glab's `--form` sends a multipart POST, and `@path` in it uploads that file.
        "gh" | "glab" => (&['X', 'f', 'F', 't'], &["--method", "--field", "--raw-field", "--input", "--form", "--show-token"], false),
        // Short forms too: -f --file, -d --destination, -s --source, -m --method.
        "az" => {
            (&['m', 'f', 'd', 's'], &["--http-method", "--method", "--in-file", "--out-file", "--output-file", "--file", "--destination", "--source"], true)
        }
        _ => return false,
    };
    // az replaces `@path`, and the value in `key=@path` or `--flag=@path`, with that file's contents.
    if program == "az" && (arg.starts_with('@') || arg.split_once('=').is_some_and(|(_, value)| value.starts_with('@'))) {
        return true;
    }
    let name = arg.split('=').next().unwrap_or(arg);
    if let Some(option) = name.strip_prefix("--") {
        // An exact option name wins over abbreviations, so az's own `--output` isn't `--output-file`.
        let exact = program == "az" && name == "--output";
        return !option.is_empty() && !exact && long.iter().any(|flag| if abbrev { flag.starts_with(name) } else { *flag == name });
    }
    // Short flags group and carry their value attached, like `-iXDELETE` or `-ccore.fsmonitor=...`.
    name.strip_prefix('-').is_some_and(|group| group.chars().any(|c| short.contains(&c)))
}

/// The only flags these commands take from a plugin, whatever its manifest says. Their other flags sign the CLI in as
/// someone else (`az login --service-principal`, `glab auth login --token`), send a host's token to another server
/// (after `glab auth login --hostname gitlab.com --api-host <other>`, every glab call for gitlab.com went there), or get
/// a token for another service (`az account get-access-token --resource-type arm`). Names are exact, so an abbreviation
/// or an attached value counts as another flag.
const ONLY_FLAGS: &[(&str, &[&str], &[&str])] = &[
    ("az", &["login"], &["--allow-no-subscriptions", "--output", "-o"]),
    ("az", &["account", "get-access-token"], &["--resource", "--output", "-o"]),
    ("az", &["rest"], &["--url", "--uri", "-u", "--resource", "--output", "-o", "--query"]),
    ("gh", &["auth", "login"], &["--web", "--clipboard", "--hostname"]),
    ("glab", &["auth", "login"], &["--web", "--hostname"]),
];

/// Commands that change a CLI's own settings, which every later call of every plugin then follows, refused whatever the
/// manifest says: they install code (`gh extension install`, `az extension add`), point a command at any program (`gh
/// alias set --shell`, `gh config set browser`, `git config core.fsmonitor`), or move where calls and tokens go (`az
/// devops configure --defaults organization=`, `az cloud set`, `glab config set token --host`).
const SETTINGS_COMMANDS: &[(&str, &[&str])] = &[
    ("az", &["config"]),
    ("az", &["configure"]),
    ("az", &["cloud"]),
    ("az", &["extension"]),
    ("az", &["devops", "configure"]),
    ("gh", &["config"]),
    ("gh", &["alias"]),
    ("gh", &["extension"]),
    ("gh", &["extensions"]),
    ("gh", &["ext"]),
    ("glab", &["config"]),
    ("glab", &["alias"]),
    ("git", &["config"]),
];

/// The CLIs whose checks find a command by its first words. Each also takes options before the command, which would
/// hide it from those checks (gh and glab ran `--hostname=x api <url>`, git runs `-C . config`), and no plugin needs one.
const COMMAND_FIRST: [&str; 4] = ["git", "gh", "glab", "az"];

/// What no manifest can allow in a CLI call: an option before its command, or a command that changes the CLI's settings.
pub(crate) fn command_refused(id: &str, program: &str, args: &[String]) -> Result<(), String> {
    if COMMAND_FIRST.contains(&program) && args.first().is_some_and(|a| a.starts_with('-')) {
        return Err(format!("{id} must start a {program} call with its command, not {}", args[0]));
    }
    match SETTINGS_COMMANDS.iter().find(|(p, words)| *p == program && starts_with_words(args, words)) {
        Some((_, words)) => Err(format!("{id} may not change {program}'s own settings with {program} {}", words.join(" "))),
        None => Ok(()),
    }
}

/// Whether `args` start with a command's words.
fn starts_with_words(args: &[String], words: &[&str]) -> bool {
    words.iter().enumerate().all(|(i, word)| args.get(i).is_some_and(|a| a == word))
}

/// A flag that a command in `ONLY_FLAGS` doesn't take.
pub(crate) fn flag_not_taken<'a>(program: &str, args: &'a [String]) -> Option<&'a String> {
    let (_, command, flags) = ONLY_FLAGS.iter().find(|(p, command, _)| *p == program && starts_with_words(args, command))?;
    args[command.len()..].iter().find(|a| a.starts_with('-') && !flags.contains(&a.split('=').next().unwrap_or(a)))
}

/// The value of each flag `is_name` matches: after its `=`, or the next word. `None` where there's none.
fn flag_values(args: &[String], is_name: impl Fn(&str) -> bool) -> Vec<Option<&str>> {
    let (mut words, mut values) = (args.iter(), Vec::new());
    while let Some(arg) = words.next() {
        let (name, inline) = arg.split_once('=').map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
        if is_name(name) {
            values.push(inline.or_else(|| words.next().map(String::as_str)));
        }
    }
    values
}

/// Whether an `az` call reaches only Azure DevOps, and only with Azure DevOps' token. The azure-devops extension before
/// 1.0.6 sends your token to whatever `--organization` names, `az rest` sends one to whatever `--url` names, and `az
/// account get-access-token` prints the token it gets. `ONLY_FLAGS` has already refused other spellings of the last two.
pub(crate) fn az_reaches_azure_devops_only(args: &[String]) -> bool {
    let all = |values: Vec<Option<&str>>, ok: fn(&str) -> bool| values.into_iter().all(|v| v.is_some_and(ok));
    let one = |values: Vec<Option<&str>>, ok: fn(&str) -> bool| values.len() == 1 && all(values, ok);
    let resource = || flag_values(args, |n| n == "--resource");
    let devops = |r: &str| r == AZURE_DEVOPS_RESOURCE;
    // az takes any unambiguous start of a long flag, so `--orga` is `--organization` too. `--or` is ambiguous with the
    // `--org` alias today, but counts too, whatever aliases an az version has.
    all(flag_values(args, |n| n.len() > 2 && "--organization".starts_with(n)), azure_devops_url)
        && match args.first().map(String::as_str) {
            Some("rest") => one(flag_values(args, |n| matches!(n, "--url" | "--uri" | "-u")), azure_devops_url) && all(resource(), devops),
            Some("account") if args.get(1).is_some_and(|a| a == "get-access-token") => one(resource(), devops),
            _ => true,
        }
}

/// An https URL on `dev.azure.com` or an `<org>.visualstudio.com` host, with a path or none, like an organization.
fn azure_devops_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let org_host = |org: &str| !org.is_empty() && org.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    matches!(rest[host.len()..].chars().next(), None | Some('/')) && (host == "dev.azure.com" || host.strip_suffix(".visualstudio.com").is_some_and(org_host))
}

/// `gh api` and `glab api` call any URL their endpoint names, and glab sends its token there (`glab api
/// https://example.invalid/x` sent `Private-Token`), so an endpoint may only be a plain path on the CLI's own host, like
/// `repos/o/r/pulls/1/comments` or `projects/g%2Fp/merge_requests?per_page=100`.
pub(crate) fn api_endpoints_ok(program: &str, args: &[String]) -> bool {
    if !matches!(program, "gh" | "glab") || args.first().is_none_or(|a| a != "api") {
        return true;
    }
    // The flags whose value is the next word; a short one takes it when it ends its group. The write flags are refused already.
    let (long, short): (&[&str], &[char]) = match program {
        "gh" => (&["--cache", "--header", "--hostname", "--jq", "--preview", "--template"], &['H', 'q', 'p']),
        _ => (&["--header", "--hostname", "--output"], &['H']),
    };
    let plain = |s: &str, extra: &[u8]| s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b) || extra.contains(&b));
    let path_ok = |endpoint: &str| {
        let (path, query) = endpoint.split_once('?').unwrap_or((endpoint, ""));
        path.split('/').all(|s| !s.is_empty() && s != "." && s != ".." && plain(s, b"")) && plain(query, b"=&")
    };
    let mut words = args[1..].iter();
    while let Some(arg) = words.next() {
        let takes_next = match arg.strip_prefix("--") {
            Some(_) => long.contains(&arg.as_str()),
            None => arg.strip_prefix('-').is_some_and(|group| group.find(|c| short.contains(&c)).is_some_and(|i| i + 1 == group.len())),
        };
        if takes_next {
            words.next();
        } else if !arg.starts_with('-') && !path_ok(arg) {
            return false;
        }
    }
    true
}

/// gh's `--jq` can read the environment (`$ENV`, `env`), which holds tokens when Wings starts from a terminal that has
/// them: `--jq '$ENV | keys | length'` printed 60 variables.
pub(crate) fn jq_reads_env(args: &[String]) -> bool {
    let reads = |jq: &str| jq.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).any(|w| w == "env" || w == "$ENV");
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        let jq = match arg.strip_prefix("--jq") {
            Some("") => words.next().map(String::as_str),
            Some(value) => value.strip_prefix('='),
            // `-q` ends a group of short flags with its value next, or has it attached: `-iq.x`.
            None => match arg.strip_prefix('-').filter(|g| !g.starts_with('-')).and_then(|g| g.split_once('q')) {
                Some((_, "")) => words.next().map(String::as_str),
                Some((_, attached)) => Some(attached),
                None => None,
            },
        };
        if jq.is_some_and(reads) {
            return true;
        }
    }
    false
}

/// `gh api graphql` runs mutations too, so a plugin's GraphQL may only read.
pub(crate) fn graphql_reads_only(fields: &serde_json::Map<String, Value>) -> bool {
    let words = |q: &str| q.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| w == "mutation" || w == "subscription");
    fields.get("query").and_then(Value::as_str).is_some_and(|q| !words(q))
}

/// git's arguments held to what a plugin may touch. `git diff <a> <b>` compares any two files once one is outside the
/// repo (`git diff /etc/hosts /dev/null` printed it), or anywhere in a folder that isn't a repo, through a symlink too
/// (`link/a link/b` printed the files `link` led to). So git needs a `cwd`, and a path must stay inside it: no `..`, and
/// what it names on disk, symlinks resolved, inside `cwd`. A path that names nothing reads nothing, like a revision
/// (`origin/main`). A fetch refspec with a destination or `+` moves a local ref (`git fetch <other repo>
/// +HEAD:refs/heads/main` put another repo's commit on main). `worktree` names its own worktrees' folders, which git
/// checks belong to the repo.
pub(crate) fn git_args_ok(args: &[String], cwd: Option<&Path>) -> bool {
    let Some(cwd) = cwd.and_then(|c| c.canonicalize().ok()) else { return false };
    let Some(command) = args.first().filter(|c| *c != "worktree") else { return true };
    let inside = |arg: &str| {
        let path = cwd.join(arg);
        let on_disk = path.canonicalize().or_else(|e| path.parent().ok_or(e).and_then(Path::canonicalize));
        on_disk.ok().is_none_or(|p| p.starts_with(&cwd))
    };
    let (mut words, mut options) = (args[1..].iter(), true);
    while let Some(arg) = words.next() {
        if options && (arg == "--" || arg == "--end-of-options") {
            options = false;
        } else if options && arg.starts_with('-') {
            // A grep pattern, not a path.
            if arg == "-e" || arg == "--regexp" {
                words.next();
            }
        } else {
            let refspec = command == "fetch" && (arg.starts_with('+') || (arg.contains(':') && !arg.contains("://")));
            if refspec || Path::new(arg).components().any(|c| c == std::path::Component::ParentDir) || !inside(arg) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        exec::{exec, exec_check, EXEC_TIMEOUT},
        panes::pane_input,
        test_support::{plugin, with_panes},
    };
    use std::fs;

    #[test]
    fn flags_that_run_code_or_write_are_refused() {
        for (program, arg) in [
            ("git", "-c"),
            ("git", "-ccore.fsmonitor=touch x"),
            ("git", "--upload-pack=sh"),
            ("git", "--output=/tmp/x"),
            ("git", "--no-index"),
            ("git", "--upload=sh"),
            ("git", "--outp=/tmp/x"),
            ("gh", "-XDELETE"),
            ("gh", "-iXDELETE"),
            ("glab", "-pfquery=mutation"),
            ("az", "--http-meth=POST"),
            ("az", "--out-file=/tmp/x"),
            ("az", "--out-f"),
            ("az", "@/etc/passwd"),
            ("az", "--body=@/etc/passwd"),
            ("az", "project=@/etc/passwd"),
            ("az", "--output-file=/tmp/x"),
            ("az", "-mPOST"),
            ("az", "-f/etc/passwd"),
            ("az", "-d"),
            ("az", "--source"),
            ("gh", "--method=POST"),
            ("gh", "-fquery=mutation"),
            ("glab", "--show-token"),
            ("glab", "--form"),
            ("glab", "--form=file=@/etc/hosts"),
            ("glab", "-t"),
            ("az", "--http-method"),
        ] {
            assert!(is_unsafe_flag(program, arg), "{program} {arg}");
        }
        for (program, arg) in [
            ("git", "-C"),
            ("git", "--cached"),
            ("git", "--quiet"),
            ("git", "--no-ext-diff"),
            ("git", "--no-color"),
            ("gh", "--json"),
            ("gh", "-i"),
            ("glab", "--paginate"),
            ("glab", "--raw"),
            ("az", "-o"),
            ("az", "--output"),
            ("az", "--source-branch"),
        ] {
            assert!(!is_unsafe_flag(program, arg), "{program} {arg}");
        }
        // The escape this guards against: a config override that runs a shell command.
        let p = plugin(Path::new("/tmp"), &["git"], &[]);
        let args = ["-c".into(), "core.fsmonitor=touch /tmp/wings-pwned".into(), "status".into()];
        assert!(exec(&p, "git", &args, None, EXEC_TIMEOUT, None).unwrap_err().contains("-c"));
    }

    #[test]
    fn git_paths_stay_inside_the_folder_it_runs_in() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
        let p = plugin(&repo, &["git diff", "git ls-files", "git show", "git fetch", "git worktree remove [--]"], &[]);
        let check = |args: &[&str]| exec_check(&p, "git", &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), Some(&repo));
        let file = repo.join("src/lib.rs").display().to_string();
        for args in [
            &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", &file][..],
            &["ls-files", "--error-unmatch", "--", &file],
            &["diff", "--no-color", "origin/main...origin/feature/x", "--", "src/a.ts"],
            &["show", "abc123:src/a.ts"],
            &["fetch", "--quiet", "--end-of-options", "origin", "main", "0123456789abcdef0123456789abcdef01234567"],
            &["fetch", "https://github.com/o/r", "main"],
            &["worktree", "remove", "--", "/Users/me/dev/app-feature"],
        ] {
            assert_eq!(check(args), Ok(()), "{args:?}");
        }
        for args in [
            // git turns on --no-index by itself when a path is outside the repo.
            &["diff", "/etc/hosts", "/dev/null"][..],
            &["diff", "--no-color", "--", "/etc/hosts", "/dev/null"],
            &["diff", "--", "../outside.txt", "src/lib.rs"],
            &["show", "abc123:../../etc/hosts"],
            &["diff", "-O", "/etc/hosts", "HEAD"],
            // A refspec with a destination or `+` moves local refs.
            &["fetch", "origin", "+HEAD:refs/heads/main"],
            &["fetch", "origin", "main:main"],
            &["fetch", "origin", "+main"],
        ] {
            assert!(check(args).is_err(), "{args:?}");
        }
        // Run for real from this crate's repo, it printed /etc/hosts.
        let args: Vec<String> = ["diff", "/etc/hosts", "/dev/null"].map(String::from).to_vec();
        assert!(exec(&p, "git", &args, Some(&repo), EXEC_TIMEOUT, None).is_err());
        assert!(exec_check(&p, "git", &["diff".into(), "HEAD".into()], None).is_err(), "git needs a folder");
        // In a folder that isn't a repo, git diff compared the files a symlink led to.
        let base = std::env::temp_dir().join(format!("wings-git-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("outside")).unwrap();
        fs::create_dir_all(base.join("project")).unwrap();
        fs::write(base.join("outside/a"), "secret a\n").unwrap();
        fs::write(base.join("outside/b"), "secret b\n").unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("project/link")).unwrap();
        let args: Vec<String> = ["diff", "link/a", "link/b"].map(String::from).to_vec();
        let out = exec(&p, "git", &args, Some(&base.join("project")), EXEC_TIMEOUT, None);
        assert!(out.as_ref().is_err(), "git diff printed {:?}", out.map(|o| o.stdout));
        fs::remove_dir_all(&base).unwrap();
    }

    /// Whether `exec`'s own checks of an az call's flags and hosts let it through.
    fn az_allowed(args: &[String]) -> bool {
        flag_not_taken("az", args).is_none() && az_reaches_azure_devops_only(args)
    }

    #[test]
    fn az_rest_only_reaches_azure_devops_with_its_token() {
        let rest = |extra: &[&str]| -> Vec<String> { ["rest"].iter().chain(extra).map(|a| a.to_string()).collect() };
        let ado = "https://dev.azure.com/o/_apis/wit/workitems?ids=1";
        assert!(az_allowed(&rest(&["--resource", AZURE_DEVOPS_RESOURCE, "--url", ado, "-o", "json"])));
        assert!(az_allowed(&rest(&["--url=https://contoso.visualstudio.com/_apis/projects"])));
        for extra in [
            &["--resource", AZURE_DEVOPS_RESOURCE, "--url", "https://example.com/x"][..],
            &["--url", "https://dev.azure.com@example.com/x"],
            &["--url", "https://dev.azure.com.example.com/x"],
            &["--url", "http://dev.azure.com/x"],
            &["--url", "/subscriptions/x"],
            &["--url", ado, "--resource", "https://management.azure.com/"],
            &["--url", ado, "--url", "https://example.com/x"],
            &["--ur", "https://example.com/x"],
            &["-uhttps://example.com/x"],
            &["--resour", "https://management.azure.com/", "--url", ado],
            &["--url"],
            &[],
            &["--url", ado, "--headers", "Host=example.com"],
        ] {
            assert!(!az_allowed(&rest(extra)), "{extra:?}");
        }
        assert!(az_allowed(&["repos".into(), "pr".into(), "list".into()]));
    }

    #[test]
    fn gh_and_glab_api_only_call_plain_paths_on_their_own_host() {
        let p = plugin(Path::new("/tmp"), &["gh api", "gh pr view", "gh auth login", "glab api", "glab auth login"], &[]);
        let check = |program: &str, args: &[&str]| exec_check(&p, program, &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), None);
        // pr-tracker's calls.
        for (program, args) in [
            ("gh", &["api", "user", "--jq", ".login"][..]),
            ("gh", &["api", "--hostname", "ghe.example.com", "repos/o/r/pulls/1/comments", "--paginate", "--slurp"]),
            ("gh", &["api", "-H", "Accept: application/vnd.github.raw", "repos/o/r/actions/jobs/1/logs"]),
            ("glab", &["api", "--hostname", "gitlab.com", "--paginate", "projects/g%2Fp/merge_requests/1/discussions?per_page=100"]),
            ("glab", &["api", "--hostname", "gitlab.com", "projects/g%2Fp/merge_requests?source_branch=feature%2Fx&state=all&order_by=updated_at&per_page=5"]),
            ("gh", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "title", "--jq", ".environment"]),
            ("gh", &["auth", "login", "--web", "--clipboard", "--hostname", "github.com"]),
            ("glab", &["auth", "login", "--web", "--hostname", "gitlab.com"]),
            // pflag gives a group's first flag that takes a value the rest of the group, or the next word if it ends it.
            ("gh", &["api", "user", "-iq", ".login"]),
            ("gh", &["api", "-iH", "Accept: x", "user"]),
        ] {
            assert_eq!(check(program, args), Ok(()), "{program} {args:?}");
        }
        for (program, args) in [
            // glab sent the gitlab.com token to whatever host a full URL named, uppercase scheme and userinfo included.
            ("glab", &["api", "https://example.invalid/api/v4/user"][..]),
            ("glab", &["api", "--hostname", "gitlab.com", "HTTPS://example.invalid/x"]),
            ("glab", &["api", "https://gitlab.com@example.invalid/x"]),
            ("gh", &["api", "http://127.0.0.1:8080/admin"]),
            ("gh", &["api", "//example.invalid/x"]),
            ("gh", &["api", "--jq", ".x", "https://example.invalid/x"]),
            ("gh", &["api", "repos/../../x"]),
            ("gh", &["api", "repos/o/r?next=https://example.invalid"]),
            // `--jq` read the environment, which can hold tokens.
            ("gh", &["api", "user", "--jq", "$ENV.GH_TOKEN"]),
            ("gh", &["api", "user", "-q", "env | keys"]),
            ("gh", &["api", "user", "--jq=env.HOME"]),
            ("gh", &["pr", "view", "1", "--json", "title", "-iqenv"]),
            ("gh", &["api", "user", "-iq", "$ENV"]),
            // `-Hq` is a header with the value `q`, so the next word is the endpoint.
            ("gh", &["api", "-Hq", "https://example.invalid/x"]),
            // A sign-in can't take a token or move where the host's token goes.
            ("glab", &["auth", "login", "--hostname", "gitlab.com", "--api-host", "example.invalid"]),
            ("glab", &["auth", "login", "--hostname", "gitlab.com", "-a", "example.invalid", "--web"]),
            ("glab", &["auth", "login", "--hostname", "gitlab.com", "--token", "x"]),
            ("gh", &["auth", "login", "--web", "--insecure-storage"]),
            ("gh", &["auth", "login", "--with-token"]),
        ] {
            assert!(check(program, args).is_err(), "{program} {args:?}");
        }
    }

    #[test]
    fn az_names_only_azure_devops_organizations() {
        // The azure-devops extension before 1.0.6 sent the az login token to any --organization: version 1.0.2 sent
        // `Authorization: Basic` with it to a listener on 127.0.0.1.
        let call = |org: &[&str]| -> Vec<String> {
            ["devops", "invoke", "--area", "core", "--resource", "projects"].iter().chain(org).map(|a| a.to_string()).collect()
        };
        for org in [
            &["--organization", "https://dev.azure.com/contoso"][..],
            &["--org", "https://dev.azure.com/contoso/"],
            &["--organization=https://contoso.visualstudio.com"],
            &[],
        ] {
            assert!(az_allowed(&call(org)), "{org:?}");
        }
        for org in [
            &["--organization", "https://example.com/contoso"][..],
            &["--organization", "http://127.0.0.1:9/contoso"],
            &["--org=https://dev.azure.com.example.com/contoso"],
            &["--orga", "https://example.com/contoso"],
            &["--or", "https://example.com/contoso"],
            &["--organization", "https://dev.azure.com@example.com/contoso"],
            &["--organization", "https://dev.azure.com/contoso", "--org", "https://example.com/x"],
            &["--organization"],
        ] {
            assert!(!az_allowed(&call(org)), "{org:?}");
        }
    }

    #[test]
    fn no_plugin_changes_a_clis_own_settings() {
        // Even a manifest that allows every command of a CLI can't change what that CLI does on later calls.
        let p = plugin(Path::new("/tmp"), &["az", "gh", "glab", "git"], &[]);
        let check = |program: &str, args: &[&str]| exec_check(&p, program, &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), None);
        for (program, args) in [
            ("az", &["devops", "configure", "--defaults", "organization=https://example.com/x"][..]),
            ("az", &["config", "set", "extension.index_url=https://example.com/index.json"]),
            ("az", &["configure", "--defaults", "group=x"]),
            ("az", &["cloud", "set", "--name", "x"]),
            ("az", &["extension", "add", "--source", "https://example.com/x.whl"]),
            ("gh", &["extension", "install", "octo/gh-x"]),
            ("gh", &["ext", "install", "octo/gh-x"]),
            ("gh", &["alias", "set", "--shell", "x", "touch /tmp/x"]),
            ("gh", &["config", "set", "browser", "touch /tmp/x"]),
            ("glab", &["config", "set", "token", "x", "--host", "gitlab.com"]),
            ("glab", &["alias", "set", "x", "!touch /tmp/x"]),
            ("git", &["config", "core.fsmonitor", "touch /tmp/x"]),
        ] {
            assert!(check(program, args).unwrap_err().contains("own settings"), "{program} {args:?}");
        }
        assert_eq!(check("az", &["devops", "invoke", "--area", "core", "--organization", "https://dev.azure.com/o"]), Ok(()));
        assert_eq!(check("gh", &["pr", "view", "1"]), Ok(()));
        // An option before the command would hide it from these checks: gh and glab ran `--hostname=x api <url>`.
        for (program, args) in [
            ("git", &["-C", ".", "config", "core.sshCommand", "touch /tmp/x"][..]),
            ("git", &["--no-pager", "config", "core.fsmonitor", "touch /tmp/x"]),
            ("gh", &["--hostname", "example.com", "api", "https://example.com/x"]),
            ("glab", &["--hostname=x", "api", "http://127.0.0.1:9/x"]),
            ("az", &["--only-show-errors", "rest", "--url", "https://example.com/"]),
        ] {
            assert!(check(program, args).unwrap_err().contains("with its command"), "{program} {args:?}");
        }
        let panes = with_panes(Some(&["git", "az"]));
        assert!(pane_input(&panes, Some("git -C . config core.pager x"), &[]).unwrap_err().contains("with its command"));
        assert!(pane_input(&panes, Some("git config core.pager x"), &[]).unwrap_err().contains("own settings"));
        // `args` are typed after the command's words, so they're checked with them.
        let typed = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(pane_input(&panes, Some("git"), &typed(&["config", "core.sshCommand", "touch /tmp/x"])).unwrap_err().contains("own settings"));
        assert!(pane_input(&panes, Some("az"), &typed(&["boards", "query", "@/etc/hosts"])).unwrap_err().contains("@/etc/hosts"));
    }

    #[test]
    fn az_gets_only_azure_devops_tokens_and_keeps_its_sign_in() {
        // Without a flag list, `az account get-access-token` printed an ARM and a Microsoft Graph token too.
        let token = |extra: &[&str]| -> Vec<String> { ["account", "get-access-token"].iter().chain(extra).map(|a| a.to_string()).collect() };
        assert!(az_allowed(&token(&["--resource", AZURE_DEVOPS_RESOURCE, "-o", "json"])));
        for extra in [
            &["--resource", "https://management.azure.com/"][..],
            &["--resource-type", "arm"],
            &["--scope", "https://graph.microsoft.com/.default"],
            &["--resource", AZURE_DEVOPS_RESOURCE, "--resource", "https://management.azure.com/"],
            &["--resource", AZURE_DEVOPS_RESOURCE, "--tenant", "t"],
            &["--resou", "https://management.azure.com/"],
            &[],
        ] {
            assert!(!az_allowed(&token(extra)), "{extra:?}");
        }
        // `az login` can't sign az in as another identity, which every later az call of every plugin would act as.
        let login = |extra: &[&str]| -> Vec<String> { ["login"].iter().chain(extra).map(|a| a.to_string()).collect() };
        assert!(az_allowed(&login(&["--allow-no-subscriptions", "--output", "none"])));
        for extra in [
            &["--service-principal", "-u", "app", "-p", "secret", "--tenant", "t"][..],
            &["--identity"],
            &["--federated-token", "x"],
            &["--allow"],
            &["-onone"],
        ] {
            assert!(!az_allowed(&login(extra)), "{extra:?}");
        }
    }
}
