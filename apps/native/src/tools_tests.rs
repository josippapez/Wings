use std::{collections::HashMap, path::PathBuf};

use super::*;

#[test]
fn a_version_is_the_first_three_numbers_in_what_a_tool_prints() {
    for (printed, version) in [
        ("gh version 2.105.0 (2026-11-02)\nhttps://github.com/cli/cli/releases/tag/v2.105.0", "2.105.0"),
        ("rtk 1.2.1", "1.2.1"),
        ("2.1.330 (Claude Code)", "2.1.330"),
        ("git version 2.39.5 (Apple Git-154)", "2.39.5"),
        ("glab 1.50.0 (abc)", "1.50.0"),
        ("ollama version is 0.5.1", "0.5.1"),
        ("v1.2.3-beta.1", "1.2.3"),
        ("2.93.0,abc123", "2.93.0"),
    ] {
        assert_eq!(parse_version(printed).as_deref(), Some(version), "{printed}");
    }
    for refused in ["", "latest", "1.2", "$(touch x).1.2", "1..2", "99999999999.1.1"] {
        assert_eq!(parse_version(refused), None, "{refused}");
    }
}

#[test]
fn a_newer_version_is_compared_by_number() {
    assert!(is_newer("2.106.0", "2.105.0"));
    assert!(is_newer("2.1.330", "2.1.99"));
    assert!(!is_newer("2.105.0", "2.105.0"));
    assert!(!is_newer("2.9.0", "2.10.0"));
    assert!(!is_newer("nonsense", "1.0.0"));
}

#[test]
fn the_source_is_read_from_where_the_program_resolves() {
    let source = |path: &str| detect_source(Path::new(path));
    assert_eq!(source("/opt/homebrew/Cellar/gh/2.105.0/bin/gh"), Source::Homebrew("gh".into()));
    assert_eq!(
        source("/opt/homebrew/Cellar/opencode-v2/1.0.0/bin/opencode"),
        Source::Homebrew("opencode-v2".into())
    );
    assert_eq!(
        source("/usr/local/Cellar/git/2.50.0/bin/git"),
        Source::Homebrew("git".into())
    );
    assert_eq!(
        source("/opt/homebrew/Caskroom/azure-cli/2.93.0/bin/az"),
        Source::HomebrewCask("azure-cli".into())
    );
    assert_eq!(
        source("/Users/me/.local/share/claude/versions/2.1.330"),
        Source::ClaudeInstaller
    );
    assert_eq!(source("/usr/bin/git"), Source::Unknown);
    // A folder name that isn't a package name is no package.
    assert_eq!(source("/opt/homebrew/Cellar/$(x)/1/bin/x"), Source::Unknown);
    assert_eq!(source("/opt/homebrew/Cellar"), Source::Unknown);
}

#[test]
fn the_update_command_is_fixed_for_each_source() {
    let command = |program, source| update_command(program, &source);
    assert_eq!(command("gh", Source::Homebrew("gh".into())).as_deref(), Some("brew upgrade gh"));
    assert_eq!(
        command("az", Source::HomebrewCask("azure-cli".into())).as_deref(),
        Some("brew upgrade --cask azure-cli")
    );
    assert_eq!(command("claude", Source::ClaudeInstaller).as_deref(), Some("claude update"));
    assert_eq!(command("gh", Source::Unknown), None);
    // A name that could carry a second command is refused even if it got this far.
    assert_eq!(command("gh", Source::Homebrew("gh; rm -rf ~".into())), None);
    assert_eq!(command("gh", Source::Homebrew("gh && x".into())), None);
}

#[test]
fn the_update_runs_a_fixed_program_with_fixed_arguments() {
    let plan = |program, source| update_plan(program, &source).map(|c| (c.program, c.args));
    let args = |program: &str, args: &[&str]| Some((program.to_owned(), args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>()));
    assert_eq!(plan("gh", Source::Homebrew("gh".into())), args("brew", &["upgrade", "gh"]));
    assert_eq!(
        plan("opencode", Source::Homebrew("anomalyco/tap/opencode-v2".into())),
        args("brew", &["upgrade", "anomalyco/tap/opencode-v2"])
    );
    assert_eq!(
        plan("az", Source::HomebrewCask("azure-cli".into())),
        args("brew", &["upgrade", "--cask", "azure-cli"])
    );
    assert_eq!(plan("claude", Source::ClaudeInstaller), args("claude", &["update"]));
    // Two slashes, an option or a space is no package.
    for bad in ["a/b", "a/b/c/d", "--all", "a/b/-c", "a b", "a/b/c;x"] {
        assert_eq!(plan("gh", Source::Homebrew(bad.into())), None, "{bad}");
    }
    assert_eq!(
        update_command("opencode", &Source::Homebrew("anomalyco/tap/opencode-v2".into())).as_deref(),
        Some("brew upgrade anomalyco/tap/opencode-v2")
    );
}

#[test]
fn a_failed_update_shows_the_last_lines_it_printed() {
    let text = "==> Upgrading gh\n\n  \nDownloading\nError: one\nError: two\nError: three\n";
    assert_eq!(last_lines(text), "Error: one\nError: two\nError: three");
    assert_eq!(last_lines(""), "");
    let long = "x".repeat(400);
    assert_eq!(last_lines(&long).chars().count(), 161);
}

const TAP_FORMULA: &str = r#"{"formulae":[{"name":"opencode-v2","full_name":"anomalyco/tap/opencode-v2","tap":"anomalyco/tap","versions":{"stable":"2.4.0"}}],"casks":[]}"#;
const CORE_FORMULA: &str = r#"{"formulae":[{"name":"gh","full_name":"gh","tap":"homebrew/core","versions":{"stable":"2.105.0"}}],"casks":[]}"#;
const TAP_CASK: &str = r#"{"formulae":[],"casks":[{"token":"thing","full_token":"someone/tap/thing","tap":"someone/tap","version":"3.1.4,abc"}]}"#;

#[test]
fn brew_info_names_a_tap_of_someone_elses_and_reads_its_newest_version() {
    let formula = Source::Homebrew("opencode-v2".into());
    assert_eq!(
        brew_info_args(&formula).unwrap(),
        ["info", "--json=v2", "opencode-v2"]
    );
    assert_eq!(
        brew_info_args(&Source::HomebrewCask("thing".into())).unwrap(),
        ["info", "--json=v2", "--cask", "thing"]
    );
    assert_eq!(brew_info_args(&Source::Unknown), None);
    assert_eq!(
        parse_brew_info(&formula, TAP_FORMULA),
        Some(BrewInfo { source: Source::Homebrew("anomalyco/tap/opencode-v2".into()), latest: Some("2.4.0".into()) })
    );
    assert_eq!(
        parse_brew_info(&Source::HomebrewCask("thing".into()), TAP_CASK),
        Some(BrewInfo { source: Source::HomebrewCask("someone/tap/thing".into()), latest: Some("3.1.4".into()) })
    );
    // Core keeps its name and its newest version comes from the API.
    let gh = Source::Homebrew("gh".into());
    assert_eq!(parse_brew_info(&gh, CORE_FORMULA), Some(BrewInfo { source: gh.clone(), latest: None }));
    assert_eq!(parse_brew_info(&gh, "Error: nope"), None);
    // A name that could be an option is refused.
    let bad = TAP_FORMULA.replace("anomalyco/tap/opencode-v2", "--force");
    assert_eq!(parse_brew_info(&formula, &bad), None);
}

#[test]
fn a_tool_from_a_tap_of_someone_elses_is_read_from_brew_and_updated_by_its_full_name() {
    let mut mac = mac();
    mac.programs.insert("brew", ("/opt/homebrew/Library/brew.sh", ""));
    mac.programs.insert("opencode", ("/opt/homebrew/Cellar/opencode-v2/2.3.0/bin/opencode", "opencode 2.3.0"));
    mac.brew.insert("opencode-v2", TAP_FORMULA);
    mac.brew.insert("gh", CORE_FORMULA);
    let tools = check(&mac, &wants(&[]));
    let get = |program: &str| tools.iter().find(|t| t.program == program).unwrap();
    let opencode = get("opencode");
    assert_eq!(opencode.source, Source::Homebrew("anomalyco/tap/opencode-v2".into()));
    assert_eq!(opencode.latest.as_deref(), Some("2.4.0"));
    assert_eq!(opencode.command().as_deref(), Some("brew upgrade anomalyco/tap/opencode-v2"));
    // The API isn't asked about what it doesn't have, and core still uses it.
    let asked = mac.asked.lock().unwrap().clone();
    assert!(asked.iter().all(|url| !url.contains("opencode")), "{asked:?}");
    assert_eq!(get("gh").source, Source::Homebrew("gh".into()));
    assert_eq!(get("gh").latest.as_deref(), Some("2.105.0"));
}

#[test]
fn the_newest_version_is_read_from_each_sources_answer() {
    let formula = Source::Homebrew("gh".into());
    assert_eq!(
        latest_url(&formula).as_deref(),
        Some("https://formulae.brew.sh/api/formula/gh.json")
    );
    assert_eq!(
        parse_latest(&formula, r#"{"name":"gh","versions":{"stable":"2.105.0","head":"HEAD"}}"#).as_deref(),
        Some("2.105.0")
    );
    let cask = Source::HomebrewCask("azure-cli".into());
    assert_eq!(
        latest_url(&cask).as_deref(),
        Some("https://formulae.brew.sh/api/cask/azure-cli.json")
    );
    assert_eq!(parse_latest(&cask, r#"{"version":"2.93.0"}"#).as_deref(), Some("2.93.0"));
    assert_eq!(parse_latest(&Source::ClaudeInstaller, "2.1.330\n").as_deref(), Some("2.1.330"));
    assert_eq!(parse_latest(&formula, "<html>"), None);
    assert_eq!(parse_latest(&Source::ClaudeInstaller, "rm -rf /"), None);
    assert_eq!(latest_url(&Source::Unknown), None);
}

fn plugin(name: &str, entries: &[&str]) -> (String, Vec<String>) {
    (name.into(), entries.iter().map(|e| (*e).into()).collect())
}

#[test]
fn only_what_the_app_and_the_plugins_run_is_listed() {
    let plugins = [
        plugin("PR tracker", &["gh pr view [--json]", "git remote get-url []"]),
        plugin("Disk usage", &["df -k -P [-k -P]", "open -R [-R]"]),
        plugin("Claude chat", &["claude --version [--version]", "../evil x"]),
    ];
    let wants = wants(&plugins);
    let get = |program: &str| wants.iter().find(|w| w.program == program).unwrap();
    assert_eq!(get("gh").plugins, ["PR tracker"]);
    assert!(get("gh").known);
    assert_eq!(get("gh").uses(), "Installing plugins from GitHub, PR tracker");
    assert_eq!(get("git").uses(), "Branches and worktrees, PR tracker");
    assert_eq!(get("claude").uses(), "Connecting Claude Code to Wings, Claude chat");
    // What the app starts is wanted with no plugin at all.
    assert_eq!(get("opencode").uses(), "Connecting OpenCode to Wings");
    // Nothing runs RTK, Ollama, GitLab or Azure's CLI here, so none is wanted.
    assert!(["rtk", "ollama", "glab", "az"].iter().all(|p| wants.iter().all(|w| w.program != *p)));
    // The ones with a name come first, then the rest of what plugins run, once each.
    let programs: Vec<_> = wants.iter().map(|w| w.program.as_str()).collect();
    assert_eq!(programs, ["claude", "opencode", "gh", "git", "df", "open"]);
    assert!(wants[..4].iter().all(|w| w.known) && wants[4..].iter().all(|w| !w.known));
}

#[test]
fn opentunnel_is_listed_by_name_while_share_runs_it() {
    let wants = wants(&[plugin("Share", &["opentunnel route add [--name]", "opentunnel route list []"])]);
    let tool = wants.iter().find(|w| w.program == "opentunnel").unwrap();
    assert_eq!((tool.name.as_str(), tool.known, tool.plugins.as_slice()), ("OpenTunnel", true, ["Share".to_owned()].as_slice()));
    // It is no part of the app: with no plugin that runs it, it isn't listed.
    assert!(super::wants(&[]).iter().all(|w| w.program != "opentunnel"));
}

#[test]
fn what_uses_a_tool_is_cut_to_three() {
    let want = Want {
        program: "git".into(),
        name: "Git".into(),
        built_in: String::new(),
        required: false,
        plugins: ["A", "B", "C", "D", "E"].map(String::from).to_vec(),
        known: true,
    };
    assert_eq!(want.uses(), "A, B, C, and 2 more");
}

/// A Mac with these programs: each is its resolved path, what it prints for `--version` and the answers of the newest
/// versions, which are asked for by URL.
#[derive(Default)]
struct Mac {
    programs: HashMap<&'static str, (&'static str, &'static str)>,
    web: HashMap<String, String>,
    asked: std::sync::Mutex<Vec<String>>,
    /// What `brew info --json=v2 [--cask] <name>` prints, by name.
    brew: HashMap<&'static str, &'static str>,
}

impl Lookup for Mac {
    fn find(&self, program: &str) -> Option<(PathBuf, PathBuf)> {
        let (resolved, _) = self.programs.get(program)?;
        Some((PathBuf::from("/opt/homebrew/bin").join(program), PathBuf::from(resolved)))
    }

    fn run(&self, program: &Path, args: &[&str]) -> Option<String> {
        let name = program.file_name()?.to_str()?;
        if name == "brew" {
            let (&asked, rest) = args.split_last()?;
            assert_eq!(&rest[..2], ["info", "--json=v2"]);
            return self.brew.get(asked).map(|printed| (*printed).to_owned());
        }
        let (_, printed) = self.programs.get(name)?;
        let expected: &[&str] = if name == "az" { &["version", "-o", "json"] } else { &["--version"] };
        (args == expected).then(|| (*printed).to_owned())
    }

    fn update(&self, _: &Path, _: &[String]) -> Result<(), String> {
        Ok(())
    }

    fn get(&self, url: &str, _: u64) -> Option<String> {
        self.asked.lock().unwrap().push(url.to_owned());
        self.web.get(url).cloned()
    }
}

fn mac() -> Mac {
    let mut mac = Mac::default();
    mac.programs.insert("gh", ("/opt/homebrew/Cellar/gh/2.105.0/bin/gh", "gh version 2.105.0 (2026-11-02)"));
    mac.programs.insert("rtk", ("/opt/homebrew/Cellar/rtk/1.2.0/bin/rtk", "rtk 1.2.0"));
    mac.programs.insert(
        "az",
        ("/opt/homebrew/Caskroom/azure-cli/2.93.0/bin/az", r#"{"azure-cli": "2.93.0", "core": "2.93.0"}"#),
    );
    mac.programs.insert("git", ("/usr/bin/git", "git version 2.39.5 (Apple Git-154)"));
    mac.programs.insert("df", ("/bin/df", "df: illegal option"));
    mac.programs.insert("dot", ("/opt/homebrew/Cellar/graphviz/12.0.0/bin/dot", "dot - graphviz version 12.0.0 (0)"));
    mac.web.insert(
        "https://formulae.brew.sh/api/formula/gh.json".into(),
        r#"{"versions":{"stable":"2.105.0"}}"#.into(),
    );
    mac.web.insert(
        "https://formulae.brew.sh/api/formula/rtk.json".into(),
        r#"{"versions":{"stable":"1.2.1"}}"#.into(),
    );
    mac.web.insert("https://formulae.brew.sh/api/cask/azure-cli.json".into(), r#"{"version":"2.93.0"}"#.into());
    mac
}

#[test]
fn each_tool_is_found_read_and_compared_with_its_newest() {
    let plugins = [
        plugin("PR tracker", &["gh pr view []", "git remote get-url []"]),
        plugin("Disk usage", &["df -k []", "dot -V []"]),
        plugin("Schedules", &["az boards query []"]),
        plugin("Claude chat", &["claude --version []"]),
    ];
    let mut mac = mac();
    mac.web.insert(
        "https://formulae.brew.sh/api/formula/gh.json".into(),
        r#"{"versions":{"stable":"2.106.0"}}"#.into(),
    );
    let tools = check(&mac, &wants(&plugins));
    let list: Vec<(&str, bool, Option<&str>, Option<&str>, bool)> = tools
        .iter()
        .map(|t| (t.program.as_str(), t.found, t.installed.as_deref(), t.latest.as_deref(), t.has_update()))
        .collect();
    assert_eq!(
        list,
        [
            // A plugin runs Claude Code, which isn't here. OpenCode isn't either, but only the app uses it.
            ("claude", false, None, None, false),
            ("gh", true, Some("2.105.0"), Some("2.106.0"), true),
            ("az", true, Some("2.93.0"), Some("2.93.0"), false),
            // Apple's own git has no newest version to compare with.
            ("git", true, Some("2.39.5"), None, false),
            // Programs the system ships (df) are left out. RTK is on this Mac, and nothing runs it.
            // This one's newest version isn't in the fake web.
            ("dot", true, Some("12.0.0"), None, false),
        ]
    );
    assert_eq!(tools[1].source.label(), "Homebrew");
    assert_eq!(tools[1].command().as_deref(), Some("brew upgrade gh"));
    assert_eq!(tools[2].source.label(), "Homebrew cask");
    assert_eq!(tools[3].source, Source::Unknown);
    assert_eq!(updates(&tools), 1);
}

#[test]
fn an_unknown_source_has_an_update_to_name_but_no_command() {
    let tool = ToolStatus {
        program: "gh".into(),
        name: "GitHub CLI".into(),
        uses: String::new(),
        found: true,
        installed: Some("2.0.0".into()),
        latest: Some("2.1.0".into()),
        source: Source::Unknown,
    };
    assert!(tool.has_update());
    assert_eq!(tool.command(), None);
    let missing = ToolStatus { found: false, installed: None, ..tool };
    assert!(!missing.has_update());
}

#[test]
fn only_urls_of_a_known_source_are_ever_asked_for() {
    let mac = mac();
    check(&mac, &wants(&[plugin("PR tracker", &["gh pr view []", "dot -V []"])]));
    let mut asked = mac.asked.lock().unwrap().clone();
    asked.sort();
    assert_eq!(
        asked,
        [
            "https://formulae.brew.sh/api/formula/gh.json",
            "https://formulae.brew.sh/api/formula/graphviz.json",
        ]
    );
}

const CHANGELOG_TEXT: &str = "# Changelog\n\nIntro line\n- not a version's note\n\n## 2.1.331\n\n- Added `/rewind`\n- Fixed a crash on resume\n  - indented detail\n\n## Unreleased\n\n- not this\n\n## 2.1.330\n\n- Plugins can ship hooks\n\n## 2.1.329\n\n- Older note\n";

#[test]
fn the_changelog_is_read_as_versions_with_their_notes() {
    let entries = parse_changelog(CHANGELOG_TEXT);
    assert_eq!(
        entries,
        [
            Entry {
                version: "2.1.331".into(),
                notes: vec!["Added `/rewind`".into(), "Fixed a crash on resume".into()]
            },
            Entry { version: "2.1.330".into(), notes: vec!["Plugins can ship hooks".into()] },
            Entry { version: "2.1.329".into(), notes: vec!["Older note".into()] },
        ]
    );
    let new: Vec<_> = unread(&entries, "2.1.329").iter().map(|e| e.version.as_str()).collect();
    assert_eq!(new, ["2.1.331", "2.1.330"]);
    assert!(unread(&entries, "2.1.331").is_empty());
}

#[test]
fn only_the_newest_versions_of_a_long_changelog_are_kept() {
    let text: String = (1..=100).map(|n| format!("## 1.0.{}\n- note\n", 101 - n)).collect();
    let entries = parse_changelog(&text);
    assert_eq!(entries.len(), 40);
    assert_eq!(entries[0].version, "1.0.100");
}

#[test]
fn a_tool_only_the_app_uses_is_missed_only_when_the_app_cannot_work_without_it() {
    let nothing = Mac::default();
    let missing = |plugins: &[(String, Vec<String>)]| -> Vec<String> {
        check(&nothing, &wants(plugins)).into_iter().map(|t| t.program).collect()
    };
    // Nothing is installed: only git, which the app needs, is missed. Claude Code, OpenCode and gh are optional.
    assert_eq!(missing(&[]), ["git"]);
    // A plugin that runs one makes it needed.
    assert_eq!(
        missing(&[plugin("Claude chat", &["claude --version []"]), plugin("PR tracker", &["gh pr view []"])]),
        ["claude", "gh", "git"]
    );
    // Installed, an optional one is listed with nothing needing it but the app.
    let mut mac = mac();
    mac.programs.insert("opencode", ("/opt/homebrew/Cellar/opencode-v2/1.0.0/bin/opencode", "opencode 1.0.0"));
    let listed: Vec<_> = check(&mac, &wants(&[])).into_iter().map(|t| (t.program, t.found)).collect();
    assert_eq!(listed, [("opencode".to_owned(), true), ("gh".into(), true), ("git".into(), true)]);
}
