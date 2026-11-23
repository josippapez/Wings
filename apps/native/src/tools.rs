//! The command-line tools Wings and its plugins run (Settings, Tools): which are installed, their version, the newest one
//! and how each was installed, so an update can be offered. Everything here is pure or goes through `Lookup`, which is
//! the only thing that touches the disk, a program or the network, so the tests use a fake.
//!
//! Nothing fetched becomes part of a command: a version is checked to be digits and dots, and the one name that comes
//! from the disk (a Homebrew package, from the folder the program resolves into) is checked to be a package name.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// The programs the app itself starts, and what for. Only what Wings runs is here: what the plugins run comes from their
/// manifests (`wants`). Each was found by looking for where it is started. The last field says whether the app can't work
/// without it: one it only uses when it is there is listed when it is installed, or when a plugin needs it.
pub const APP_USES: [(&str, &str, bool); 4] = [
    // `workspace/claude.rs` and `register.rs` find it to connect Claude Code to Wings' tools and hooks.
    ("claude", "Connecting Claude Code to Wings", false),
    // `workspace/opencode.rs` finds it to connect OpenCode.
    ("opencode", "Connecting OpenCode to Wings", false),
    // `projects.rs`, `sidebar/git.rs` and `wings-spaces` run it for branches, worktrees and a project's repo.
    ("git", "Branches and worktrees", true),
    // `wings-plugins/src/store.rs` asks it for your GitHub token to install a plugin from a private repo.
    ("gh", "Installing plugins from GitHub", false),
];

/// Names for the programs people know by an acronym. Any other program is listed under its own name.
const NAMES: [(&str, &str); 6] = [
    ("claude", "Claude Code"),
    ("opencode", "OpenCode"),
    ("gh", "GitHub CLI"),
    ("glab", "GitLab CLI"),
    ("az", "Azure CLI"),
    ("git", "Git"),
];

/// Where the newest Claude Code version is published, as plain text.
pub const CLAUDE_LATEST: &str = "https://downloads.claude.ai/claude-code-releases/latest";
/// Claude Code's changelog.
pub const CHANGELOG: &str =
    "https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md";
/// How much of the changelog is read, and how many of its newest versions are kept.
pub const MAX_CHANGELOG: u64 = 2 * 1024 * 1024;
const KEPT_VERSIONS: usize = 40;

/// How a program was installed, which decides where its newest version is asked and how it is updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A Homebrew formula, by the name of its folder in the Cellar.
    Homebrew(String),
    /// A Homebrew cask, by the name of its folder in the Caskroom.
    HomebrewCask(String),
    /// Claude Code's own installer.
    ClaudeInstaller,
    Unknown,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Homebrew(_) => "Homebrew",
            Source::HomebrewCask(_) => "Homebrew cask",
            Source::ClaudeInstaller => "Claude installer",
            Source::Unknown => "Unknown source",
        }
    }
}

/// A package name as Homebrew writes one: lower case letters, digits and `@ . _ + -`, starting with a letter or digit.
fn is_package(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"@._+-".contains(&b))
}

/// A formula or cask as `brew` names it: a package name, or `user/tap/name` for one from a tap of someone else's.
fn is_brew_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    matches!(parts.len(), 1 | 3) && parts.iter().all(|part| is_package(part))
}

/// How the program at `resolved` (its path with the symlinks followed) was installed.
pub fn detect_source(resolved: &Path) -> Source {
    let parts: Vec<&str> = resolved
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let after = |folder: &str| {
        parts
            .iter()
            .position(|part| *part == folder)
            .and_then(|at| parts.get(at + 1))
            .copied()
            .filter(|name| is_package(name))
            .map(str::to_owned)
    };
    if let Some(name) = after("Cellar") {
        return Source::Homebrew(name);
    }
    if let Some(name) = after("Caskroom") {
        return Source::HomebrewCask(name);
    }
    // `~/.local/share/claude/versions/<version>`.
    if parts.windows(3).any(|w| w == ["share", "claude", "versions"]) {
        return Source::ClaudeInstaller;
    }
    Source::Unknown
}

/// A program and its arguments, run as they are: nothing goes through a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

impl Command {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
        }
    }

    /// The line to type in a terminal. Only fixed words and a checked package name are in it.
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What updates `program` to its newest version. `None` when Wings doesn't know how the program was installed. Only
/// fixed words and a checked package name go into it.
pub fn update_plan(program: &str, source: &Source) -> Option<Command> {
    match source {
        Source::Homebrew(formula) if is_brew_name(formula) => {
            Some(Command::new("brew", &["upgrade", formula]))
        }
        Source::HomebrewCask(cask) if is_brew_name(cask) => {
            Some(Command::new("brew", &["upgrade", "--cask", cask]))
        }
        Source::ClaudeInstaller if program == "claude" => Some(Command::new("claude", &["update"])),
        _ => None,
    }
}

/// The line that updates `program`, typed into a terminal.
pub fn update_command(program: &str, source: &Source) -> Option<String> {
    update_plan(program, source).map(|plan| plan.line())
}

/// Where the newest version of a program installed by `source` is told, and how it is read from the answer.
pub fn latest_url(source: &Source) -> Option<String> {
    match source {
        // The API only has homebrew/core and homebrew/cask; a tap of someone else's is read with `brew_info`.
        Source::Homebrew(formula) if is_package(formula) => {
            Some(format!("https://formulae.brew.sh/api/formula/{formula}.json"))
        }
        Source::HomebrewCask(cask) if is_package(cask) => {
            Some(format!("https://formulae.brew.sh/api/cask/{cask}.json"))
        }
        Source::ClaudeInstaller => Some(CLAUDE_LATEST.into()),
        _ => None,
    }
}

/// The newest version in the answer from `latest_url`.
pub fn parse_latest(source: &Source, body: &str) -> Option<String> {
    match source {
        Source::Homebrew(_) => {
            let json: serde_json::Value = serde_json::from_str(body).ok()?;
            parse_version(json["versions"]["stable"].as_str()?)
        }
        Source::HomebrewCask(_) => {
            let json: serde_json::Value = serde_json::from_str(body).ok()?;
            parse_version(json["version"].as_str()?)
        }
        Source::ClaudeInstaller => parse_version(body.trim()),
        Source::Unknown => None,
    }
}

/// What `brew info` says about a formula or cask: the name `brew` wants for it, and its newest version when it is from a
/// tap of someone else's, which the API doesn't cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrewInfo {
    pub source: Source,
    pub latest: Option<String>,
}

/// The arguments that ask `brew` about `source`, whose answer `parse_brew_info` reads. `None` for a source that isn't
/// Homebrew's.
pub fn brew_info_args(source: &Source) -> Option<Vec<String>> {
    let args = |extra: &[&str], name: &String| {
        let mut args: Vec<String> = ["info", "--json=v2"].map(String::from).into();
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        args.push(name.clone());
        Some(args)
    };
    match source {
        Source::Homebrew(name) if is_brew_name(name) => args(&[], name),
        Source::HomebrewCask(name) if is_brew_name(name) => args(&["--cask"], name),
        _ => None,
    }
}

/// `brew info --json=v2`'s answer for `source`. From homebrew/core or homebrew/cask the source stays as it is and the
/// newest version is the API's; from any other tap the source takes the full name (`user/tap/name`), which `brew upgrade`
/// needs, and the newest version is the one in the tap's local metadata.
pub fn parse_brew_info(source: &Source, body: &str) -> Option<BrewInfo> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let (entry, full, newest, core) = match source {
        Source::Homebrew(_) => {
            let entry = &json["formulae"][0];
            (entry, &entry["full_name"], &entry["versions"]["stable"], "homebrew/core")
        }
        Source::HomebrewCask(_) => {
            let entry = &json["casks"][0];
            (entry, &entry["full_token"], &entry["version"], "homebrew/cask")
        }
        _ => return None,
    };
    if entry["tap"].as_str()? == core {
        return Some(BrewInfo { source: source.clone(), latest: None });
    }
    let full = full.as_str().filter(|name| is_brew_name(name))?.to_owned();
    Some(BrewInfo {
        source: match source {
            Source::Homebrew(_) => Source::Homebrew(full),
            _ => Source::HomebrewCask(full),
        },
        latest: newest.as_str().and_then(parse_version),
    })
}

/// The first `x.y.z` in `text`, as those three numbers: what `--version` prints, or a version a server sends. Anything
/// else in the text is ignored, so a version that isn't digits and dots never gets through.
pub fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|word| {
        let word = word.trim_start_matches(['v', 'V', '[', '(']);
        let end = word
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(word.len());
        let numbers: Vec<&str> = word[..end].trim_end_matches('.').split('.').collect();
        (numbers.len() >= 3
            && numbers[..3]
                .iter()
                .all(|n| !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit())))
        .then(|| numbers[..3].join("."))
    })
}

fn numbers(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.').map(|n| n.parse::<u64>().ok());
    Some([parts.next()??, parts.next()??, parts.next()??])
}

/// Whether `latest` is a newer version than `installed`. A version that isn't `x.y.z` is never newer.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    matches!((numbers(latest), numbers(installed)), (Some(l), Some(i)) if l > i)
}

/// One tool's row in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolStatus {
    pub program: String,
    pub name: String,
    /// What uses it, as a sentence fragment: the names of the plugins whose permissions run it, and Wings itself.
    pub uses: String,
    /// Whether it was found on this Mac.
    pub found: bool,
    /// Its version, when it is installed and printed one.
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub source: Source,
}

impl ToolStatus {
    pub fn has_update(&self) -> bool {
        match (&self.latest, &self.installed) {
            (Some(latest), Some(installed)) => self.found && is_newer(latest, installed),
            _ => false,
        }
    }

    /// What the Update button runs, when there is an update and a way to run it.
    pub fn plan(&self) -> Option<Command> {
        self.has_update()
            .then(|| update_plan(&self.program, &self.source))
            .flatten()
    }

    /// The line for a terminal, when there is one.
    pub fn command(&self) -> Option<String> {
        self.plan().map(|plan| plan.line())
    }
}

/// How many of `tools` have an update, for the count beside Tools in the list of sections.
pub fn updates(tools: &[ToolStatus]) -> usize {
    tools.iter().filter(|tool| tool.has_update()).count()
}

/// A program to look for and what uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Want {
    pub program: String,
    pub name: String,
    /// What Wings itself uses it for.
    pub built_in: String,
    /// The app can't work without it, so it is listed when it is missing too.
    pub required: bool,
    /// The plugins that run it.
    pub plugins: Vec<String>,
    /// A program with a name of its own (`NAMES`), which is listed when it is missing. Another might be part of the
    /// system, so it is listed only when it is found somewhere else.
    pub known: bool,
}

impl Want {
    fn uses(&self) -> String {
        let mut uses: Vec<&str> = self.plugins.iter().map(String::as_str).collect();
        if !self.built_in.is_empty() {
            uses.insert(0, &self.built_in);
        }
        match uses.len() {
            0 => String::new(),
            1..=3 => uses.join(", "),
            n => format!("{}, and {} more", uses[..3].join(", "), n - 3),
        }
    }
}

/// The program a permission entry runs: `gh pr view [--json]` runs `gh`.
fn program_of(entry: &str) -> Option<&str> {
    let program = entry.split_whitespace().next()?;
    (!program.contains('/')
        && program
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
    .then_some(program)
}

/// What to look for: the programs the app starts and the ones the plugins run, and nothing else. `plugins` is each
/// turned-on plugin's name and the permission entries that run programs (`exec`, `processes` and `panes`). The ones with
/// a name of their own come first.
pub fn wants(plugins: &[(String, Vec<String>)]) -> Vec<Want> {
    let programs: BTreeSet<&str> = APP_USES
        .iter()
        .map(|(program, ..)| *program)
        .chain(plugins.iter().flat_map(|(_, entries)| entries.iter().filter_map(|e| program_of(e))))
        .collect();
    let mut wants: Vec<Want> = programs
        .into_iter()
        .map(|program| {
            let name = NAMES.iter().find(|(known, _)| *known == program).map(|(_, name)| *name);
            let used = APP_USES.iter().find(|(used, ..)| *used == program);
            Want {
                program: program.into(),
                name: name.unwrap_or(program).into(),
                built_in: used.map_or("", |(_, what, _)| what).into(),
                required: used.is_some_and(|(.., required)| *required),
                plugins: plugins
                    .iter()
                    .filter(|(_, entries)| entries.iter().any(|e| program_of(e) == Some(program)))
                    .map(|(name, _)| name.clone())
                    .collect(),
                known: name.is_some(),
            }
        })
        .collect();
    let rank = |want: &Want| NAMES.iter().position(|(known, _)| *known == want.program).unwrap_or(NAMES.len());
    wants.sort_by_key(rank);
    wants
}

/// What a check reads from this Mac and the network.
pub trait Lookup: Sync {
    /// The program on the search path, and the path it resolves to with its symlinks followed.
    fn find(&self, program: &str) -> Option<(PathBuf, PathBuf)>;
    /// What the program prints when run with `args`.
    fn run(&self, program: &Path, args: &[&str]) -> Option<String>;
    /// The body at an https `url`, up to `max` bytes.
    fn get(&self, url: &str, max: u64) -> Option<String>;
    /// Runs `args` on `program` to the end, with nothing to read from, and says how it went: the last lines it printed when
    /// it didn't work.
    fn update(&self, program: &Path, args: &[String]) -> Result<(), String>;
}

/// This Mac: the search path Wings' plugins use, the programs on it and the three hosts the newest versions come from.
pub struct System;

const HOSTS: [&str; 3] = [
    "formulae.brew.sh",
    "downloads.claude.ai",
    "raw.githubusercontent.com",
];

/// How long an update may run, and how much of what it prints is kept.
const UPDATE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const UPDATE_OUTPUT: u64 = 256 * 1024;
const SHOWN_LINES: usize = 3;
const SHOWN_WIDTH: usize = 160;

/// The last few lines `text` has, for a short error: blank lines dropped, each cut to a readable width.
pub fn last_lines(text: &str) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.char_indices().nth(SHOWN_WIDTH) {
            Some((at, _)) => format!("{}…", &line[..at]),
            None => line.to_owned(),
        })
        .collect();
    lines[lines.len().saturating_sub(SHOWN_LINES)..].join("\n")
}

impl Lookup for System {
    fn find(&self, program: &str) -> Option<(PathBuf, PathBuf)> {
        let path = wings_plugins::exec::find_program(program)?;
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        Some((path, resolved))
    }

    fn run(&self, program: &Path, args: &[&str]) -> Option<String> {
        use std::{
            io::Read,
            process::{Command, Stdio},
            time::Duration,
        };
        let mut child = Command::new(program)
            .args(args)
            .env("PATH", wings_plugins::exec::search_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = (&mut stdout).take(64 * 1024).read_to_string(&mut text);
            let _ = send.send(text);
        });
        // `az` takes a while to start.
        let text = receive.recv_timeout(Duration::from_secs(30)).ok();
        let _ = child.kill();
        let _ = child.wait();
        text
    }

    fn update(&self, program: &Path, args: &[String]) -> Result<(), String> {
        use std::{
            io::Read,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut child = Command::new(program)
            .args(args)
            .env("PATH", wings_plugins::exec::search_path())
            // Nothing to answer: Homebrew skips its prompts and its hints.
            .env("NONINTERACTIVE", "1")
            .env("HOMEBREW_NO_ENV_HINTS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Couldn't start it: {error}"))?;
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(pipe) = pipe {
                    let mut bytes = Vec::new();
                    let _ = pipe.take(UPDATE_OUTPUT).read_to_end(&mut bytes);
                    text = String::from_utf8_lossy(&bytes).into_owned();
                }
                text
            })
        };
        let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let deadline = Instant::now() + UPDATE_LIMIT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let output = format!("{}\n{}", out.join().unwrap_or_default(), err.join().unwrap_or_default());
        match status {
            Some(status) if status.success() => Ok(()),
            Some(_) => Err(last_lines(&output)),
            None => Err(format!("Gave up after {} minutes.\n{}", UPDATE_LIMIT.as_secs() / 60, last_lines(&output))),
        }
    }

    fn get(&self, url: &str, max: u64) -> Option<String> {
        let uri: ureq::http::Uri = url.parse().ok()?;
        if uri.scheme_str() != Some("https") || !HOSTS.contains(&uri.host()?) {
            return None;
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        agent
            .get(url)
            .call()
            .ok()?
            .body_mut()
            .with_config()
            .limit(max)
            .read_to_string()
            .ok()
    }
}

/// The folders of programs the system ships. A program a plugin runs from one of these is part of macOS, not a tool to
/// keep up to date.
fn is_system(resolved: &Path) -> bool {
    ["/bin", "/sbin", "/usr/bin", "/usr/sbin", "/usr/libexec", "/System"]
        .iter()
        .any(|dir| resolved.starts_with(dir))
}

/// What `program --version` prints, as a version. `az` prints its own slowly, so it is asked for its JSON.
fn installed_version(lookup: &dyn Lookup, program: &str, path: &Path) -> Option<String> {
    if program == "az" {
        let json: serde_json::Value =
            serde_json::from_str(&lookup.run(path, &["version", "-o", "json"])?).ok()?;
        return parse_version(json["azure-cli"].as_str()?);
    }
    parse_version(&lookup.run(path, &["--version"])?)
}

fn check_one(lookup: &dyn Lookup, want: &Want) -> Option<ToolStatus> {
    let status = |found, installed, latest, source| ToolStatus {
        program: want.program.clone(),
        name: want.name.clone(),
        uses: want.uses(),
        found,
        installed,
        latest,
        source,
    };
    let Some((path, resolved)) = lookup.find(&want.program) else {
        // Missing is said of a program with a name of its own (another might be part of the system) that the app can't
        // work without or a plugin runs. What the app only uses when it is there isn't missed.
        return (want.known && (want.required || !want.plugins.is_empty()))
            .then(|| status(false, None, None, Source::Unknown));
    };
    if !want.known && is_system(&resolved) {
        return None;
    }
    let mut source = detect_source(&resolved);
    let installed = installed_version(lookup, &want.program, &path);
    // Which tap it came from decides its name for `brew upgrade` and where its newest version is read.
    let tap_latest = brew_info_args(&source).and_then(|args| {
        let (brew, _) = lookup.find("brew")?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let info = parse_brew_info(&source, &lookup.run(&brew, &args)?)?;
        source = info.source;
        info.latest
    });
    let latest = tap_latest.or_else(|| {
        latest_url(&source)
            .and_then(|url| lookup.get(&url, 1024 * 1024))
            .and_then(|body| parse_latest(&source, &body))
    });
    Some(status(true, installed, latest, source))
}

/// Looks for each of `wants`, all at once, in the order given.
pub fn check(lookup: &dyn Lookup, wants: &[Want]) -> Vec<ToolStatus> {
    std::thread::scope(|scope| {
        let found: Vec<_> = wants
            .iter()
            .map(|want| scope.spawn(move || check_one(lookup, want)))
            .collect();
        found
            .into_iter()
            .filter_map(|work| work.join().ok().flatten())
            .collect()
    })
}

/// One version's notes in Claude Code's changelog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub version: String,
    /// The `- ` lines under the version's heading, without the dash.
    pub notes: Vec<String>,
}

/// The newest versions of the changelog, newest first: each `## <version>` heading and the `- ` lines under it. A heading
/// that isn't a version, and indented lines under a note, are skipped.
pub fn parse_changelog(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipping = true;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            match parse_version(heading) {
                Some(version) => {
                    if entries.len() == KEPT_VERSIONS {
                        break;
                    }
                    entries.push(Entry {
                        version,
                        notes: Vec::new(),
                    });
                    skipping = false;
                }
                None => skipping = true,
            }
        } else if !skipping
            && let Some(note) = line.strip_prefix("- ")
            && let Some(entry) = entries.last_mut()
        {
            let note = note.trim();
            if !note.is_empty() {
                entry.notes.push(note.to_owned());
            }
        }
    }
    entries
}

/// The entries newer than `read`, the version last marked read.
pub fn unread<'a>(entries: &'a [Entry], read: &str) -> Vec<&'a Entry> {
    entries
        .iter()
        .filter(|entry| is_newer(&entry.version, read))
        .collect()
}

/// The first of the user's projects that holds the Claude chat plugin's source, which is the Wings repo.
pub fn wings_repo(projects: &[PathBuf]) -> Option<PathBuf> {
    projects
        .iter()
        .find(|path| path.join("plugins/claude-chat/wings-plugin.json").is_file())
        .cloned()
}

/// What Claude chat is asked in a Wings project: compare the changes with what the plugin supports.
pub fn compatibility_prompt(unread: &[&Entry]) -> String {
    let mut prompt = String::from(
        "Claude Code has released new versions. Compare the changelog entries below with the Claude chat plugin in \
         plugins/claude-chat and list what Claude chat doesn't support yet: new commands, settings, tools, message or \
         stream types, hooks, permission modes and anything else a chat would need to show or send. For each, say \
         where in plugins/claude-chat it would go. Don't change any files.\n",
    );
    for entry in unread.iter().take(15) {
        prompt.push_str(&format!("\n## {}\n", entry.version));
        for note in entry.notes.iter().take(40) {
            prompt.push_str(&format!("- {note}\n"));
        }
    }
    prompt
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
