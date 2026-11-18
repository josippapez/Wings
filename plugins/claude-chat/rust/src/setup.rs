//! What the `/hooks`, `/memory` and `/permissions` dialogs show, from Claude Code's answers to the reads the Enter that
//! typed the command sends. They only read. Claude Code's control protocol has no request that changes a hook, a memory
//! setting or a permission rule: its own `edit-hook`, `edit-memory-settings` and `edit-permission-rules` commands take
//! their edit on stdin, so a change is made in a terminal.

use serde_json::{Value, json};

use crate::session::{MODES, request};

/// The ids of the dialogs' reads.
pub const HOOKS: &str = "hooks-listing";
pub const MEMORY: &str = "memory-dialog";
pub const SIZES: &str = "memory-sizes";
pub const RULES: &str = "permission-rules";
pub const MODE: &str = "permission-mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Hooks,
    Memory,
    Permissions,
}

/// The dialog a draft opens, when the draft is exactly its command. `/allowed-tools` is Claude Code's other name for
/// `/permissions`.
pub fn command(text: &str) -> Option<Which> {
    match text.trim() {
        "/hooks" => Some(Which::Hooks),
        "/memory" => Some(Which::Memory),
        "/permissions" | "/allowed-tools" => Some(Which::Permissions),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hook {
    pub event: String,
    /// The tools or sources it runs for, empty for all.
    pub matcher: String,
    /// What runs: a command, or a prompt or URL as `kind` says.
    pub text: String,
    pub kind: String,
    /// Where it is set, and Claude Code's longer word for that, with the file.
    pub from: String,
    pub from_detail: String,
    /// The session's mode or a policy keeps it from running.
    pub off: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hooks {
    /// The events with a hook, in the order they happen, each with what it is.
    pub events: Vec<(String, String)>,
    pub hooks: Vec<Hook>,
    /// Why hooks don't run, and the settings files skipped, as (tone, text).
    pub notices: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    /// Claude Code's name for the row, like "User instructions" or a path in the project.
    pub label: String,
    pub description: String,
    pub path: String,
    pub exists: bool,
    /// How deep in imports it is: 0 for a file Claude Code reads by itself, 1 for one it imports.
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub label: String,
    pub path: String,
}

/// A memory Claude saved in the auto-memory folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    /// `user`, `feedback`, `project` or `reference`.
    pub kind: Option<String>,
    pub modified: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Memory {
    pub files: Vec<File>,
    pub folders: Vec<Folder>,
    pub saved: Vec<Saved>,
    /// What Claude Code says of auto memory: "on", "off", or why it is off.
    pub auto: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// `allow`, `ask` or `deny`.
    pub behavior: String,
    pub rule: String,
    /// Claude Code's reading of it, as (words, the rule's own part, words): "Any Bash command starting with " "npm".
    pub reading: Option<(String, String, String)>,
    pub source: String,
    /// Your organization's settings allow only their own rules, so this one is ignored.
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    pub rules: Vec<Rule>,
    pub notices: Vec<(&'static str, String)>,
}

/// The mode a session starts in by your settings: the mode, None when no settings file sets one, and which does.
#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub mode: Option<String>,
    pub from: Option<String>,
}

/// The open dialog and what it shows. Each answer is None until Claude Code gives it, or why it couldn't.
#[derive(Debug, Default)]
pub struct Setup {
    pub open: Option<Which>,
    pub hooks: Option<Result<Hooks, String>>,
    pub memory: Option<Result<Memory, String>>,
    /// The tokens each instruction file takes in the context, by path, as /context counts them.
    pub sizes: Vec<(String, u64)>,
    pub rules: Option<Result<Rules, String>>,
    pub mode: Option<Mode>,
    /// The permissions dialog's list, `allow`, `ask` or `deny`, and its search.
    pub behavior: String,
    pub query: String,
    /// The path the memory dialog copied last.
    pub copied: Option<String>,
}

impl Setup {
    /// Opens the dialog the draft `text` names, and gives the reads that fill it, which go from the Enter that sent it.
    pub fn open(&mut self, text: &str) -> Option<String> {
        let which = command(text)?;
        self.open = Some(which);
        Some(match which {
            Which::Hooks => {
                self.hooks = None;
                request(HOOKS, json!({ "subtype": "get_hooks_listing" }))
            }
            Which::Memory => {
                (self.memory, self.sizes, self.copied) = (None, Vec::new(), None);
                [request(MEMORY, json!({ "subtype": "get_memory_dialog" })), request(SIZES, json!({ "subtype": "get_context_usage" }))].concat()
            }
            Which::Permissions => {
                (self.rules, self.mode, self.behavior) = (None, None, "allow".into());
                self.query.clear();
                [request(RULES, json!({ "subtype": "list_permission_rules" })), request(MODE, json!({ "subtype": "get_settings" }))].concat()
            }
        })
    }

    /// Takes in Claude Code's answer `response` to the request `id`. False when it isn't one of these dialogs'.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        let answer = match response["subtype"] == "success" {
            true => Ok(&response["response"]),
            false => Err(response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned()),
        };
        match id {
            HOOKS => self.hooks = Some(answer.map(hooks_of)),
            MEMORY => self.memory = Some(answer.map(memory_of)),
            SIZES => self.sizes = answer.map(sizes_of).unwrap_or_default(),
            RULES => self.rules = Some(answer.map(rules_of)),
            // Without it the dialog lists the rules alone.
            MODE => self.mode = answer.ok().map(mode_of),
            _ => return false,
        }
        true
    }

    /// The tokens the file at `path` takes in the context, when Claude Code has loaded it.
    pub fn size(&self, path: &str) -> Option<u64> {
        self.sizes.iter().find(|(file, _)| file == path).map(|(_, tokens)| *tokens)
    }
}

/// Where a hook or a rule is set, as the chat says it.
pub fn source_label(source: &str) -> &str {
    match source {
        "userSettings" => "User settings",
        "projectSettings" => "Project settings",
        "localSettings" => "Local settings",
        "policySettings" => "Managed settings",
        "flagSettings" => "Startup settings",
        "cliArg" => "Command line",
        "command" => "Command configuration",
        "session" | "sessionHook" => "This session",
        "pluginHook" => "Plugin",
        "builtinHook" => "Built-in",
        "toolsNarrowing" => "CLI tool narrowing",
        "mcpServerPolicy" => "MCP server policy",
        "hostCredential" => "Cloud-session credential guard",
        "mcpMount" => "Remote Control server mount",
        other => other,
    }
}

/// The file a settings source is, for its tooltip.
pub fn source_file(source: &str) -> Option<&'static str> {
    match source {
        "userSettings" => Some("~/.claude/settings.json"),
        "projectSettings" => Some(".claude/settings.json"),
        "localSettings" => Some(".claude/settings.local.json"),
        _ => None,
    }
}

/// A permission mode as the mode picker names it.
pub fn mode_name(mode: &str) -> &str {
    MODES.iter().find(|(value, _)| *value == mode).map_or(mode, |(_, name)| name)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn list(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

/// The settings files Claude Code skipped, each as a notice.
fn skipped(errors: &Value) -> impl Iterator<Item = (&'static str, String)> + '_ {
    list(errors).map(|error| match error["file"].as_str() {
        Some(file) => ("danger", format!("Claude Code skipped {file}: {}", text(error, "message"))),
        None => ("danger", text(error, "message").to_owned()),
    })
}

fn hooks_of(answer: &Value) -> Hooks {
    let events = list(&answer["events"]).map(|event| (text(event, "name").to_owned(), text(event, "summary").to_owned())).collect();
    let hooks = list(&answer["hooks"])
        .map(|hook| {
            let source = text(hook, "source");
            let from = match (source, hook["pluginName"].as_str()) {
                ("pluginHook", Some(plugin)) => format!("Plugin {plugin}"),
                _ => source_label(source).to_owned(),
            };
            let what = hook["commandText"].as_str().unwrap_or(text(hook, "displayText"));
            Hook {
                event: text(hook, "event").to_owned(),
                matcher: text(hook, "matcher").to_owned(),
                text: what.to_owned(),
                kind: text(hook, "contentLabel").to_owned(),
                from,
                from_detail: text(hook, "sourceLabel").to_owned(),
                off: hook["disabled"] == true,
            }
        })
        .collect();
    let policy = &answer["policy"];
    let mut notices = Vec::new();
    if policy["disabledByPolicy"] == true {
        notices.push(("warn", "Your organization's settings turn every hook off, so none of these run.".to_owned()));
    } else if policy["allDisabled"] == true {
        notices.push(("warn", "disableAllHooks is set in your settings, so none of these run.".to_owned()));
    }
    if policy["managedOnly"] == true {
        notices.push(("info", "Only hooks from your organization's settings run here. Your own aren't listed, and theirs aren't shown either.".to_owned()));
    }
    if let Some(safe) = answer["safeMode"].as_object() {
        notices.push(("warn", format!("Safe mode: hooks from settings files don't run this session. {}", safe.get("exitHint").and_then(Value::as_str).unwrap_or_default())));
    }
    if let Some(bare) = answer["bareMode"].as_object() {
        notices.push(("warn", format!("Bare mode: hooks from settings files don't run. {}", bare.get("exitHint").and_then(Value::as_str).unwrap_or_default())));
    }
    notices.extend(skipped(&answer["errors"]));
    Hooks { events, hooks, notices }
}

fn memory_of(answer: &Value) -> Memory {
    let files = list(&answer["files"])
        .map(|file| {
            // An import is labelled "L <path>", two spaces before it for each level past the first.
            let label = text(file, "label");
            let bare = label.trim_start();
            let (label, depth) = match bare.strip_prefix("L ") {
                Some(path) => (path, (label.len() - bare.len()) / 2 + 1),
                None => (bare, 0),
            };
            File { label: label.to_owned(), description: text(file, "description").to_owned(), path: text(file, "path").to_owned(), exists: file["exists"] != false, depth }
        })
        .collect();
    let folders = list(&answer["folders"])
        .map(|folder| {
            let label = match text(folder, "kind") {
                "auto" => "Auto-memory folder".to_owned(),
                "team" => "Team memory folder".to_owned(),
                // "Open <agent> agent memory": the terminal's row opens it, and here the path is copied instead.
                _ => text(folder, "label").trim_start_matches("Open ").to_owned(),
            };
            Folder { label, path: text(folder, "path").to_owned() }
        })
        .collect();
    let saved = list(&answer["memories"])
        .map(|memory| Saved {
            name: text(memory, "name").to_owned(),
            path: text(memory, "path").to_owned(),
            description: memory["description"].as_str().map(str::to_owned),
            kind: memory["type"].as_str().map(str::to_owned),
            modified: memory["modified_ms"].as_f64().unwrap_or(0.0),
        })
        .collect();
    Memory { files, folders, saved, auto: text(&answer["auto_memory"], "status").to_owned() }
}

fn sizes_of(answer: &Value) -> Vec<(String, u64)> {
    list(&answer["memoryFiles"]).filter_map(|file| Some((file["path"].as_str()?.to_owned(), file["tokens"].as_u64()?))).collect()
}

fn rules_of(answer: &Value) -> Rules {
    let state = &answer["state"];
    let rules = list(&state["rules"])
        .map(|rule| Rule {
            behavior: text(rule, "behavior").to_owned(),
            rule: text(rule, "rule").to_owned(),
            reading: rule["description"]
                .as_object()
                .map(|reading| [reading.get("prefix"), reading.get("emphasis"), reading.get("suffix")].map(|part| part.and_then(Value::as_str).unwrap_or_default().to_owned()))
                .map(|[prefix, emphasis, suffix]| (prefix, emphasis, suffix)),
            source: text(rule, "source").to_owned(),
            ignored: rule["notInEffect"] == true,
        })
        .collect();
    let mut notices = Vec::new();
    if state["managedOnly"] == true {
        notices.push(("info", "Your organization's settings allow only their own rules, so the others are listed but not in effect.".to_owned()));
    }
    if state["withheld"] == true {
        notices.push(("info", "Claude Code cut this list short for this connection.".to_owned()));
    }
    notices.extend(skipped(&state["errors"]));
    Rules { rules, notices }
}

fn mode_of(answer: &Value) -> Mode {
    // The sources come from the lowest priority up, so the last that sets it decides it.
    let from = answer["sources"].as_array().into_iter().flatten().rev().find(|source| source["settings"]["permissions"]["defaultMode"].is_string()).map(|source| source_label(text(source, "source")).to_owned());
    Mode { mode: answer["effective"]["permissions"]["defaultMode"].as_str().map(str::to_owned), from }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    /// Claude Code 2.1.327's answers to the dialogs' reads, in its shapes, with made-up paths, hooks and rules.
    const ANSWERS: &str = include_str!("../tests/setup.jsonl");

    fn heard() -> Session {
        let mut session = Session::default();
        for line in ANSWERS.lines() {
            session.hear(&serde_json::from_str(line).unwrap());
        }
        session
    }

    fn subtypes(lines: &str) -> Vec<String> {
        lines.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()["request"]["subtype"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn hooks_memory_and_permissions_open_their_dialog_ask_only_to_read_and_are_not_sent_as_messages() {
        for (typed, which, reads) in [
            ("/hooks", Which::Hooks, vec!["get_hooks_listing"]),
            (" /memory\n", Which::Memory, vec!["get_memory_dialog", "get_context_usage"]),
            ("/permissions", Which::Permissions, vec!["list_permission_rules", "get_settings"]),
            ("/allowed-tools", Which::Permissions, vec!["list_permission_rules", "get_settings"]),
        ] {
            let mut setup = Setup::default();
            let lines = setup.open(typed).expect(typed);
            assert_eq!(setup.open, Some(which), "{typed}");
            assert_eq!(subtypes(&lines), reads, "{typed}");
            assert!(lines.lines().all(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "control_request"), "{typed}");
        }
        for typed in ["/hooks list", "/memory add a note", "/permission", "hooks", "/plugins"] {
            assert_eq!(command(typed), None, "{typed}");
            assert_eq!(Setup::default().open(typed), None, "{typed}");
        }
    }

    #[test]
    fn the_hooks_answer_lists_each_hook_under_its_event_with_where_it_is_set() {
        let session = heard();
        let hooks = session.setup.hooks.clone().expect("an answer").expect("the hooks");
        let events: Vec<&str> = hooks.events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(events, ["PreToolUse", "PostToolUse", "SessionStart", "Stop"]);
        assert_eq!(hooks.events[3].1, "Right before Claude concludes its response");
        let shown: Vec<(&str, &str, &str, &str)> = hooks.hooks.iter().map(|hook| (hook.event.as_str(), hook.matcher.as_str(), hook.text.as_str(), hook.from.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("PreToolUse", "Bash", "~/.claude/hooks/guard-rm.sh", "User settings"),
                ("PreToolUse", "Bash", "${CLAUDE_PROJECT_DIR}/.claude/hooks/no-deploy.sh", "Project settings"),
                ("PostToolUse", "Edit|Write", "pnpm prettier --write \"$CLAUDE_FILE_PATHS\"", "Project settings"),
                // What runs, not the status line a plugin shows while it does.
                ("SessionStart", "startup", "node ${CLAUDE_PLUGIN_ROOT}/hooks/rules.cjs", "Plugin toolbox"),
                ("Stop", "", "Check that the tests ran before you stop.", "Local settings"),
            ]
        );
        assert_eq!((hooks.hooks[4].kind.as_str(), hooks.hooks[0].from_detail.as_str()), ("Prompt", "User settings (~/.claude/settings.json)"));
        assert!(hooks.hooks.iter().all(|hook| !hook.off) && hooks.notices.is_empty());
    }

    #[test]
    fn hooks_that_dont_run_are_marked_and_say_why() {
        let mut setup = Setup::default();
        let answer = json!({ "subtype": "success", "request_id": HOOKS, "response": {
            "events": [{ "name": "PreToolUse", "summary": "Before tool execution" }],
            "hooks": [{ "event": "PreToolUse", "matcher": "Bash", "source": "userSettings", "commandText": "echo hi", "contentLabel": "Command", "disabled": true }],
            "policy": { "disabledByPolicy": false, "managedOnly": false, "allDisabled": true },
            "safeMode": { "managedHooksStillApply": false, "exitHint": "Start without --safe-mode" },
            "errors": [{ "file": "/work/shop/.claude/settings.local.json", "path": "", "message": "Invalid or malformed JSON" }] } });
        assert!(setup.answered(HOOKS, &answer));
        let hooks = setup.hooks.expect("an answer").expect("the hooks");
        assert!(hooks.hooks[0].off);
        let tones: Vec<&str> = hooks.notices.iter().map(|(tone, _)| *tone).collect();
        assert_eq!(tones, ["warn", "warn", "danger"]);
        assert!(hooks.notices[0].1.contains("disableAllHooks"));
        assert!(hooks.notices[1].1.ends_with("Start without --safe-mode"));
        assert_eq!(hooks.notices[2].1, "Claude Code skipped /work/shop/.claude/settings.local.json: Invalid or malformed JSON");
    }

    #[test]
    fn the_memory_answer_lists_the_files_with_their_size_and_the_saved_memories() {
        let session = heard();
        let setup = &session.setup;
        let memory = setup.memory.clone().expect("an answer").expect("the memory");
        let files: Vec<(&str, &str, usize, Option<u64>)> = memory.files.iter().map(|file| (file.label.as_str(), file.path.as_str(), file.depth, setup.size(&file.path))).collect();
        assert_eq!(
            files,
            [
                ("User instructions", "/home/me/.claude/CLAUDE.md", 0, Some(412)),
                ("Project instructions", "/work/shop/CLAUDE.md", 0, Some(1830)),
                ("docs/style.md", "/work/shop/docs/style.md", 1, Some(260)),
                (".claude/rules/testing.md", "/work/shop/.claude/rules/testing.md", 0, Some(126)),
                ("CLAUDE.local.md", "/work/shop/CLAUDE.local.md", 0, Some(38)),
            ]
        );
        assert_eq!(memory.files[2].description, "@-imported");
        assert_eq!(memory.folders, [Folder { label: "Auto-memory folder".into(), path: "/home/me/.claude/projects/-work-shop/memory/".into() }]);
        let saved: Vec<(&str, Option<&str>)> = memory.saved.iter().map(|saved| (saved.name.as_str(), saved.kind.as_deref())).collect();
        assert_eq!(saved, [("MEMORY.md", None), ("deploy_steps.md", Some("project")), ("feedback_tests.md", Some("feedback"))]);
        assert_eq!(setup.size(&memory.saved[0].path), Some(18));
        assert_eq!(memory.auto, "on");
        // The context's own numbers stay the chat's: this read is the dialog's.
        assert_eq!(session.context, None);
    }

    #[test]
    fn an_import_two_levels_down_and_a_file_not_made_yet_read_as_claude_code_lists_them() {
        let mut setup = Setup::default();
        let answer = json!({ "subtype": "success", "request_id": MEMORY, "response": { "files": [
            { "kind": "project", "path": "/work/shop/CLAUDE.md", "label": "Project instructions", "description": "Saved in ./CLAUDE.md", "exists": false },
            { "kind": "other", "path": "/work/shop/docs/deep.md", "label": "  L docs/deep.md", "description": "@-imported", "exists": true },
        ], "folders": [{ "kind": "agent", "path": "/home/me/.claude/agent-memory/reviewer/", "label": "Open reviewer agent memory", "description": "user scope" }],
            "auto_memory": { "enabled": false, "toggleable": false, "status": "off in safe mode" } } });
        assert!(setup.answered(MEMORY, &answer));
        let memory = setup.memory.expect("an answer").expect("the memory");
        assert_eq!((memory.files[0].exists, memory.files[1].depth, memory.files[1].label.as_str()), (false, 2, "docs/deep.md"));
        assert_eq!(memory.folders[0].label, "reviewer agent memory");
        assert!(memory.saved.is_empty());
        assert_eq!(memory.auto, "off in safe mode");
    }

    #[test]
    fn the_permissions_answer_lists_the_rules_with_their_source_and_the_default_mode_with_its() {
        let session = heard();
        let rules = session.setup.rules.clone().expect("an answer").expect("the rules");
        let shown: Vec<(&str, &str, &str)> = rules.rules.iter().map(|rule| (rule.behavior.as_str(), rule.rule.as_str(), source_label(&rule.source))).collect();
        assert_eq!(
            shown,
            [
                ("allow", "Bash(npm run test:*)", "User settings"),
                ("allow", "Read(~/notes/**)", "User settings"),
                ("allow", "Bash(pnpm lint)", "Project settings"),
                ("allow", "WebFetch(domain:example.com)", "Local settings"),
                ("allow", "mcp__files__read_file", "This session"),
                ("ask", "Bash(git push:*)", "Project settings"),
                ("deny", "Bash(rm -rf:*)", "User settings"),
                ("deny", "WebFetch(domain:pastebin.com)", "Managed settings"),
            ]
        );
        assert_eq!(rules.rules[0].reading, Some(("Any Bash command starting with ".into(), "npm run test".into(), String::new())));
        assert_eq!(rules.rules[1].reading, None);
        assert!(rules.notices.is_empty());
        assert_eq!(session.setup.mode, Some(Mode { mode: Some("acceptEdits".into()), from: Some("User settings".into()) }));
        assert_eq!(mode_name("acceptEdits"), "Accept edits");
        assert_eq!(mode_name("bypassPermissions"), "bypassPermissions");
    }

    #[test]
    fn rules_your_organization_overrides_are_marked_and_no_mode_set_reads_as_none() {
        let mut setup = Setup::default();
        let rules = json!({ "subtype": "success", "request_id": RULES, "response": { "state": {
            "rules": [{ "behavior": "allow", "source": "userSettings", "rule": "Bash(ls)", "editability": "readonly", "notInEffect": true }],
            "workspaceDirectories": [], "originalCwd": "/work/shop", "managedOnly": true } } });
        assert!(setup.answered(RULES, &rules));
        let rules = setup.rules.clone().expect("an answer").expect("the rules");
        assert!(rules.rules[0].ignored);
        assert_eq!(rules.notices.len(), 1);
        let settings = json!({ "subtype": "success", "request_id": MODE, "response": { "effective": { "permissions": { "allow": ["Bash(ls)"] } },
            "sources": [{ "source": "userSettings", "settings": { "permissions": { "allow": ["Bash(ls)"] } } }] } });
        assert!(setup.answered(MODE, &settings));
        assert_eq!(setup.mode, Some(Mode { mode: None, from: None }));
    }

    #[test]
    fn a_refused_read_says_why_and_opening_again_reads_anew() {
        let mut setup = Setup::default();
        setup.open("/hooks");
        let refused = json!({ "subtype": "error", "request_id": HOOKS, "error": "get_hooks_listing is not available on this connection" });
        assert!(setup.answered(HOOKS, &refused));
        assert_eq!(setup.hooks, Some(Err("get_hooks_listing is not available on this connection".into())));
        assert!(!setup.answered("plugins-settings", &refused), "another dialog's answer");
        setup.open("/hooks");
        assert_eq!(setup.hooks, None, "loading again");
        setup.open("/permissions");
        let refused = json!({ "subtype": "error", "request_id": MODE, "error": "boom" });
        assert!(setup.answered(MODE, &refused));
        assert_eq!(setup.mode, None);
    }
}
