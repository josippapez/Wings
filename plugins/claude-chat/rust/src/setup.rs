//! What the `/hooks`, `/memory` and `/permissions` dialogs show, from Claude Code's answers to the reads the Enter that
//! typed the command sends, and the changes they make. Claude Code's control protocol has no request that changes a
//! hook, a memory setting or a permission rule, so a change runs one of its own commands, `claude edit-hook`,
//! `edit-memory-settings` or `edit-permission-rules`, which reads the edit as JSON on stdin and writes the settings file
//! itself. Each change is one you confirmed with a click, after the dialog showed what it changes and in which file.

use std::{cell::RefCell, rc::Rc};

use serde_json::{Value, json};
use wings_plugin::host;

use crate::session::{MODES, request};

/// The ids of the dialogs' reads.
pub const HOOKS: &str = "hooks-listing";
pub const MEMORY: &str = "memory-dialog";
pub const SIZES: &str = "memory-sizes";
pub const RULES: &str = "permission-rules";
pub const MODE: &str = "permission-mode";
/// The memory dialog's read that /hooks and /permissions make too, for the folder your user settings are in.
pub const FOLDER: &str = "settings-folder";

/// The settings files the edit commands write. They refuse any other source.
pub const FILES: [&str; 3] = ["localSettings", "projectSettings", "userSettings"];

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
    /// Claude Code's record of it in a settings file, which names it to `edit-hook`: the file, its raw matcher and its
    /// config. None for a hook a plugin, your organization or the session sets.
    pub stored: Option<(String, String, Value)>,
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
    /// Whether auto memory is on, when Claude Code lets you turn it on or off.
    pub auto_toggle: Option<bool>,
    /// What Claude Code says of auto dream, when it shows it, and whether it is on when you may turn it on or off.
    pub dream: Option<(String, Option<bool>)>,
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
    /// It is in a settings file `edit-permission-rules` may change.
    pub editable: bool,
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

/// A change the dialogs make to Claude Code's settings, with Claude Code's own command for it.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Adds `rule` to the `behavior` rules (`allow`, `ask` or `deny`) of the settings `file`, one of `FILES`.
    AddRule { behavior: String, rule: String, file: String },
    RemoveRule { behavior: String, rule: String, file: String },
    /// Removes the `event` hook Claude Code keeps in `file` under the raw `matcher` as `config`. `text` is what it runs.
    RemoveHook { event: String, file: String, matcher: String, config: Value, text: String },
    /// Sets `autoMemoryEnabled` or `autoDreamEnabled`, which Claude Code keeps in your user settings.
    Memory { setting: &'static str, on: bool },
}

impl Edit {
    /// Removes `rule` from its settings file. None for a rule the edit command can't change.
    pub fn remove_rule(rule: &Rule) -> Option<Edit> {
        rule.editable.then(|| Edit::RemoveRule { behavior: rule.behavior.clone(), rule: rule.rule.clone(), file: rule.source.clone() })
    }

    /// Removes `hook` from its settings file. None for a hook that isn't in one.
    pub fn remove_hook(hook: &Hook) -> Option<Edit> {
        let (file, matcher, config) = hook.stored.clone()?;
        Some(Edit::RemoveHook { event: hook.event.clone(), file, matcher, config, text: hook.text.clone() })
    }

    /// The `claude` subcommand that makes it, and the edit it reads on stdin, as Claude Code 2.1.327 takes them.
    pub fn command(&self) -> (&'static str, Value) {
        match self {
            Edit::AddRule { behavior, rule, file } => ("edit-permission-rules", json!({ "op": "add", "rules": [rule], "behavior": behavior, "destination": file })),
            Edit::RemoveRule { behavior, rule, file } => ("edit-permission-rules", json!({ "op": "remove", "rule": rule, "behavior": behavior, "source": file })),
            Edit::RemoveHook { event, file, matcher, config, .. } => {
                let target = json!({ "event": event, "matcher": matcher, "hook": config });
                ("edit-hook", json!({ "op": "remove", "source": file, "event": event, "matcher": matcher, "target": target }))
            }
            Edit::Memory { setting, on } => ("edit-memory-settings", json!({ *setting: on })),
        }
    }

    /// What its confirm shows: the question, what it adds or removes as it is written, and the file it changes.
    /// `user_dir` is the folder your user settings are in, once Claude Code has said which.
    pub fn shown(&self, user_dir: Option<&str>) -> (String, String, String) {
        let place = |file: &str| place(file, user_dir);
        match self {
            Edit::AddRule { behavior, rule, file } => (format!("Add this {behavior} rule?"), rule.clone(), format!("To {}", place(file))),
            Edit::RemoveRule { behavior, rule, file } => (format!("Remove this {behavior} rule?"), rule.clone(), format!("From {}", place(file))),
            Edit::RemoveHook { event, file, matcher, text, .. } => {
                let title = if matcher.is_empty() { format!("Remove this {event} hook?") } else { format!("Remove this {event} hook for {matcher}?") };
                (title, text.clone(), format!("From {}", place(file)))
            }
            Edit::Memory { setting, on } => (format!("Turn {} {}?", memory_name(setting), on_off(*on)), format!("\"{setting}\": {on}"), format!("In {}", place("userSettings"))),
        }
    }

    /// The label of the button that makes it.
    pub fn verb(&self) -> String {
        match self {
            Edit::AddRule { behavior, .. } => format!("Add {behavior} rule"),
            Edit::RemoveRule { .. } | Edit::RemoveHook { .. } => "Remove".into(),
            Edit::Memory { on, .. } => format!("Turn {}", on_off(*on)),
        }
    }

    /// What it did, once Claude Code has made it.
    fn done(&self) -> String {
        match self {
            Edit::AddRule { behavior, rule, file } => format!("Added {rule} to the {behavior} rules in {}.", source_label(file)),
            Edit::RemoveRule { behavior, rule, file } => format!("Removed {rule} from the {behavior} rules in {}.", source_label(file)),
            Edit::RemoveHook { event, file, .. } => format!("Removed the {event} hook from {}.", source_label(file)),
            Edit::Memory { setting, on } => format!("Turned {} {}.", memory_name(setting), on_off(*on)),
        }
    }

    /// The read that lists what it changed again.
    pub fn reread(&self) -> String {
        match self {
            Edit::AddRule { .. } | Edit::RemoveRule { .. } => request(RULES, json!({ "subtype": "list_permission_rules" })),
            Edit::RemoveHook { .. } => request(HOOKS, json!({ "subtype": "get_hooks_listing" })),
            Edit::Memory { .. } => request(MEMORY, json!({ "subtype": "get_memory_dialog" })),
        }
    }
}

/// A settings file as the confirm names it: "Local settings (.claude/settings.local.json)".
pub fn place(file: &str, user_dir: Option<&str>) -> String {
    format!("{} ({})", source_label(file), source_path(file, user_dir).unwrap_or_default())
}

fn memory_name(setting: &str) -> &'static str {
    if setting == "autoDreamEnabled" { "auto dream" } else { "auto memory" }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Makes `edit` with Claude Code's own command for it, under the Claude account of the chat's `session` and in the chat's
/// folder `cwd`, where Claude Code finds the project's settings files. Every change the dialogs make runs through here.
/// It gives the warnings Claude Code printed, or why it refused.
pub async fn run(session: Option<&str>, cwd: &str, edit: &Edit) -> Result<Vec<String>, String> {
    let (command, input) = edit.command();
    let printed = Rc::new(RefCell::new(Value::Null));
    let kept = printed.clone();
    let account = host::Account::of_session(session);
    let process = host::spawn_as(&account, "claude", &[command, "--json"], cwd, &input.to_string(), move |line| *kept.borrow_mut() = line.clone());
    // It reads stdin to its end before it does anything.
    process.input().stop();
    let exit = process.await;
    outcome(&exit, &printed.borrow())
}

/// How an edit command ended: the warnings it printed with `{"ok":true}`, or the line it printed to stderr when it
/// refused, which says why.
pub fn outcome(exit: &Result<Value, String>, printed: &Value) -> Result<Vec<String>, String> {
    let exit = exit.as_ref().map_err(String::clone)?;
    if exit["code"] == 0 {
        return Ok(list(&printed["warnings"]).filter_map(|warning| warning.as_str().map(str::to_owned)).collect());
    }
    match exit["stderr"].as_str().unwrap_or_default().lines().rev().map(str::trim).find(|line| !line.is_empty()) {
        Some(said) => Err(said.to_owned()),
        None => Err(format!("claude exited with code {}.", exit["code"])),
    }
}

/// The new permission rule you are writing.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleForm {
    pub rule: String,
    pub behavior: String,
    pub file: String,
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
    /// The change you asked for, which the dialog shows with what it changes until you confirm or cancel it, and
    /// whether Claude Code is making it.
    pub pending: Option<Edit>,
    pub saving: bool,
    /// How the last change went, as notices: what it did and Claude Code's warnings, or why it refused.
    pub outcome: Vec<(&'static str, String)>,
    /// The permissions dialog's form for a new rule, while it is open.
    pub form: Option<RuleForm>,
    /// The folder your user settings are in, which differs from `~/.claude` under another Claude account.
    pub user_dir: Option<String>,
}

impl Setup {
    /// Opens the dialog the draft `text` names, and gives the reads that fill it, which go from the Enter that sent it.
    pub fn open(&mut self, text: &str) -> Option<String> {
        let which = command(text)?;
        self.open = Some(which);
        (self.pending, self.form) = (None, None);
        self.outcome.clear();
        Some(match which {
            Which::Hooks => {
                self.hooks = None;
                [request(HOOKS, json!({ "subtype": "get_hooks_listing" })), request(FOLDER, json!({ "subtype": "get_memory_dialog" }))].concat()
            }
            Which::Memory => {
                (self.memory, self.sizes, self.copied) = (None, Vec::new(), None);
                [request(MEMORY, json!({ "subtype": "get_memory_dialog" })), request(SIZES, json!({ "subtype": "get_context_usage" }))].concat()
            }
            Which::Permissions => {
                (self.rules, self.mode, self.behavior) = (None, None, "allow".into());
                self.query.clear();
                [
                    request(RULES, json!({ "subtype": "list_permission_rules" })),
                    request(MODE, json!({ "subtype": "get_settings" })),
                    request(FOLDER, json!({ "subtype": "get_memory_dialog" })),
                ]
                .concat()
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
            MEMORY => {
                self.user_dir = answer.as_ref().ok().and_then(|answer| user_dir_of(answer)).or(self.user_dir.take());
                self.memory = Some(answer.map(memory_of));
            }
            // Without it the confirms name the default account's folder.
            FOLDER => self.user_dir = answer.ok().and_then(user_dir_of).or(self.user_dir.take()),
            SIZES => self.sizes = answer.map(sizes_of).unwrap_or_default(),
            RULES => self.rules = Some(answer.map(rules_of)),
            // Without it the dialog lists the rules alone.
            MODE => self.mode = answer.ok().map(mode_of),
            _ => return false,
        }
        true
    }

    /// What Claude Code says of where a hook is set, with the user settings file as it is under this account.
    pub fn where_set(&self, detail: &str) -> String {
        match &self.user_dir {
            Some(dir) => detail.replace("~/.claude/settings.json", &format!("{}/settings.json", dir.trim_end_matches('/'))),
            None => detail.to_owned(),
        }
    }

    /// The tokens the file at `path` takes in the context, when Claude Code has loaded it.
    pub fn size(&self, path: &str) -> Option<u64> {
        self.sizes.iter().find(|(file, _)| file == path).map(|(_, tokens)| *tokens)
    }

    /// Shows `edit` with what it changes, for you to confirm. Nothing is written yet.
    pub fn ask(&mut self, edit: Edit) {
        if !self.saving {
            self.pending = Some(edit);
            self.outcome.clear();
        }
    }

    /// Drops the change you asked for, unless Claude Code is already making it.
    pub fn cancel(&mut self) {
        if !self.saving {
            self.pending = None;
        }
    }

    /// The edit your click on the confirm button makes: the one shown, once, and none while another is being made.
    pub fn confirm(&mut self) -> Option<Edit> {
        if self.saving {
            return None;
        }
        let edit = self.pending.clone()?;
        self.saving = true;
        Some(edit)
    }

    /// Takes in how Claude Code made `edit`: the warnings it gave, or why it refused. A rule it added closes its form,
    /// and a refused one leaves the form as you wrote it.
    pub fn saved(&mut self, edit: &Edit, outcome: Result<Vec<String>, String>) {
        self.saving = false;
        self.pending = None;
        self.outcome = match outcome {
            Ok(warnings) => {
                if matches!(edit, Edit::AddRule { .. }) {
                    self.form = None;
                }
                std::iter::once(("success", edit.done())).chain(warnings.into_iter().map(|warning| ("warn", warning))).collect()
            }
            Err(why) => vec![("danger", why)],
        };
    }

    /// Opens the form for a new rule of the list on screen, saved in this project's local settings unless you pick
    /// another file. The rule starts empty, so only what you type goes in it.
    pub fn new_rule(&mut self) {
        self.form = Some(RuleForm { rule: String::new(), behavior: self.behavior.clone(), file: "localSettings".into() });
        self.outcome.clear();
    }

    /// Asks to add the rule the form holds, once it has one.
    pub fn review_rule(&mut self) {
        let Some(form) = &self.form else {
            return;
        };
        let rule = form.rule.trim();
        if !rule.is_empty() {
            let edit = Edit::AddRule { behavior: form.behavior.clone(), rule: rule.to_owned(), file: form.file.clone() };
            self.ask(edit);
        }
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

/// The file a settings source is. Your user settings are in the folder of the Claude account the chat runs under, which is
/// `user_dir` once known (Claude Code's own labels say `~/.claude` whatever the account), else the default account's.
pub fn source_path(source: &str, user_dir: Option<&str>) -> Option<String> {
    match (source, user_dir) {
        ("userSettings", Some(dir)) => Some(format!("{}/settings.json", dir.trim_end_matches('/'))),
        _ => source_file(source).map(str::to_owned),
    }
}

/// The folder Claude Code keeps your user settings in, from the memory dialog's answer: the user instructions file is the
/// first row it lists, `CLAUDE.md` in that folder.
fn user_dir_of(answer: &Value) -> Option<String> {
    let file = list(&answer["files"]).find(|file| file["kind"] == "user")?;
    file["path"].as_str()?.strip_suffix("/CLAUDE.md").filter(|dir| !dir.is_empty()).map(str::to_owned)
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
            // Only a hook in a settings file has it. Its matcher is the raw one, which `matcher` shows escaped.
            let editable = &hook["editable"];
            let stored = (editable.is_object() && FILES.contains(&source)).then(|| (source.to_owned(), text(editable, "matcher").to_owned(), editable["config"].clone()));
            Hook {
                event: text(hook, "event").to_owned(),
                matcher: text(hook, "matcher").to_owned(),
                text: what.to_owned(),
                kind: text(hook, "contentLabel").to_owned(),
                from,
                from_detail: text(hook, "sourceLabel").to_owned(),
                off: hook["disabled"] == true,
                stored,
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
    let toggle = |state: &Value| (state["toggleable"] == true).then(|| state["enabled"] == true);
    let dream = &answer["auto_dream"];
    Memory {
        files,
        folders,
        saved,
        auto: text(&answer["auto_memory"], "status").to_owned(),
        auto_toggle: toggle(&answer["auto_memory"]),
        dream: (dream["shown"] == true).then(|| (text(dream, "status").to_owned(), toggle(dream))),
    }
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
            editable: rule["editability"] == "persistent" && FILES.contains(&text(rule, "source")),
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
    use wings_plugin::task;

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
            ("/hooks", Which::Hooks, vec!["get_hooks_listing", "get_memory_dialog"]),
            (" /memory\n", Which::Memory, vec!["get_memory_dialog", "get_context_usage"]),
            ("/permissions", Which::Permissions, vec!["list_permission_rules", "get_settings", "get_memory_dialog"]),
            ("/allowed-tools", Which::Permissions, vec!["list_permission_rules", "get_settings", "get_memory_dialog"]),
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
        assert_eq!((memory.auto_toggle, memory.dream), (None, None), "nothing to turn on or off");
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
        assert_eq!(Edit::remove_rule(&rules.rules[0]), None, "Claude Code says it can't be changed here");
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

    fn add(behavior: &str, rule: &str, file: &str) -> Edit {
        Edit::AddRule { behavior: behavior.into(), rule: rule.into(), file: file.into() }
    }

    #[test]
    fn each_change_is_the_json_its_edit_command_takes() {
        assert_eq!(
            add("allow", "Bash(npm test)", "localSettings").command(),
            ("edit-permission-rules", json!({ "op": "add", "rules": ["Bash(npm test)"], "behavior": "allow", "destination": "localSettings" }))
        );
        let session = heard();
        let rules = session.setup.rules.clone().unwrap().unwrap();
        assert_eq!(
            Edit::remove_rule(&rules.rules[5]).expect("a project rule").command(),
            ("edit-permission-rules", json!({ "op": "remove", "rule": "Bash(git push:*)", "behavior": "ask", "source": "projectSettings" }))
        );
        // A hook is named by Claude Code's own record of it, which the listing gives.
        let hooks = session.setup.hooks.clone().unwrap().unwrap();
        let target = json!({ "event": "PreToolUse", "matcher": "Bash", "hook": { "type": "command", "command": "~/.claude/hooks/guard-rm.sh", "timeout": 5 } });
        assert_eq!(
            Edit::remove_hook(&hooks.hooks[0]).expect("a user hook").command(),
            ("edit-hook", json!({ "op": "remove", "source": "userSettings", "event": "PreToolUse", "matcher": "Bash", "target": target }))
        );
        let target = json!({ "event": "Stop", "matcher": "", "hook": { "type": "prompt", "prompt": "Check that the tests ran before you stop." } });
        assert_eq!(Edit::remove_hook(&hooks.hooks[4]).unwrap().command().1, json!({ "op": "remove", "source": "localSettings", "event": "Stop", "matcher": "", "target": target }));
        assert_eq!(Edit::Memory { setting: "autoMemoryEnabled", on: false }.command(), ("edit-memory-settings", json!({ "autoMemoryEnabled": false })));
        assert_eq!(Edit::Memory { setting: "autoDreamEnabled", on: true }.command(), ("edit-memory-settings", json!({ "autoDreamEnabled": true })));
    }

    #[test]
    fn rows_a_plugin_your_organization_or_the_session_set_offer_no_change() {
        let session = heard();
        let rules = session.setup.rules.clone().unwrap().unwrap();
        let removable: Vec<&str> = rules.rules.iter().filter(|rule| Edit::remove_rule(rule).is_some()).map(|rule| rule.rule.as_str()).collect();
        // Not the session's mcp__files__read_file, nor the managed pastebin rule.
        assert_eq!(removable, ["Bash(npm run test:*)", "Read(~/notes/**)", "Bash(pnpm lint)", "WebFetch(domain:example.com)", "Bash(git push:*)", "Bash(rm -rf:*)"]);
        let hooks = session.setup.hooks.clone().unwrap().unwrap();
        let removable: Vec<bool> = hooks.hooks.iter().map(|hook| Edit::remove_hook(hook).is_some()).collect();
        assert_eq!(removable, [true, true, true, false, true], "the plugin's SessionStart hook stays");
        let memory = session.setup.memory.clone().unwrap().unwrap();
        assert_eq!((memory.auto_toggle, memory.dream), (Some(true), None), "auto memory turns off, and auto dream isn't shown");
        // Auto dream shows once Claude Code shows it, with a switch only while it may change.
        let mut setup = Setup::default();
        let shown = |toggleable: bool| json!({ "subtype": "success", "request_id": MEMORY, "response": { "files": [],
            "auto_memory": { "enabled": true, "toggleable": true, "status": "on" },
            "auto_dream": { "shown": true, "enabled": false, "toggleable": toggleable, "status": "off", "detail": "" } } });
        setup.answered(MEMORY, &shown(true));
        assert_eq!(setup.memory.clone().unwrap().unwrap().dream, Some(("off".into(), Some(false))));
        setup.answered(MEMORY, &shown(false));
        assert_eq!(setup.memory.clone().unwrap().unwrap().dream, Some(("off".into(), None)));
    }

    #[test]
    fn the_confirm_says_what_changes_and_in_which_file() {
        let rule = add("allow", "Bash(npm test)", "localSettings");
        assert_eq!(rule.shown(None), ("Add this allow rule?".into(), "Bash(npm test)".into(), "To Local settings (.claude/settings.local.json)".into()));
        assert_eq!(rule.verb(), "Add allow rule");
        let rules = heard().setup.rules.unwrap().unwrap();
        assert_eq!(
            Edit::remove_rule(&rules.rules[6]).unwrap().shown(None),
            ("Remove this deny rule?".into(), "Bash(rm -rf:*)".into(), "From User settings (~/.claude/settings.json)".into())
        );
        let hooks = heard().setup.hooks.unwrap().unwrap();
        let hook = Edit::remove_hook(&hooks.hooks[1]).unwrap();
        assert_eq!(hook.shown(None), ("Remove this PreToolUse hook for Bash?".into(), "${CLAUDE_PROJECT_DIR}/.claude/hooks/no-deploy.sh".into(), "From Project settings (.claude/settings.json)".into()));
        assert_eq!(hook.verb(), "Remove");
        let off = Edit::Memory { setting: "autoMemoryEnabled", on: false };
        assert_eq!(off.shown(None), ("Turn auto memory off?".into(), "\"autoMemoryEnabled\": false".into(), "In User settings (~/.claude/settings.json)".into()));
        assert_eq!(off.verb(), "Turn off");
    }

    #[test]
    fn under_another_account_the_confirm_names_that_accounts_user_settings() {
        // `get_memory_dialog` lists the user instructions file in the account's folder (probed with a scratch CLAUDE_CONFIG_DIR),
        // while the hooks listing's own label says `~/.claude` whatever the account.
        let mut setup = Setup::default();
        setup.open("/hooks");
        let answer = json!({ "subtype": "success", "request_id": FOLDER, "response": { "files": [
            { "kind": "user", "path": "/accounts/work/CLAUDE.md", "label": "User instructions", "description": "Saved in ~/.claude/CLAUDE.md", "exists": true },
            { "kind": "project", "path": "/work/shop/CLAUDE.md", "label": "Project instructions", "description": "Checked in at ./CLAUDE.md", "exists": false },
        ] } });
        assert!(setup.answered(FOLDER, &answer));
        assert_eq!(setup.user_dir.as_deref(), Some("/accounts/work"));
        let off = Edit::Memory { setting: "autoMemoryEnabled", on: false };
        assert_eq!(off.shown(setup.user_dir.as_deref()).2, "In User settings (/accounts/work/settings.json)");
        let local = add("allow", "Bash(npm test)", "localSettings");
        assert_eq!(local.shown(setup.user_dir.as_deref()).2, "To Local settings (.claude/settings.local.json)");
        assert_eq!(setup.where_set("User settings (~/.claude/settings.json)"), "User settings (/accounts/work/settings.json)");
        // Before the folder is known, or when the read fails, it is the default account's.
        assert_eq!(off.shown(None).2, "In User settings (~/.claude/settings.json)");
        setup.answered(FOLDER, &json!({ "subtype": "error", "request_id": FOLDER, "error": "no" }));
        assert_eq!(setup.user_dir.as_deref(), Some("/accounts/work"));
    }

    #[test]
    fn a_new_rule_starts_empty_and_is_only_shown_until_you_confirm_it() {
        let mut setup = Setup::default();
        setup.open("/permissions");
        setup.behavior = "deny".into();
        setup.new_rule();
        // Nothing of Claude's: the rule is yours to type.
        assert_eq!(setup.form, Some(RuleForm { rule: String::new(), behavior: "deny".into(), file: "localSettings".into() }));
        setup.review_rule();
        assert_eq!(setup.pending, None, "an empty rule asks nothing");
        setup.form.as_mut().unwrap().rule = "  Read(./secrets/**) ".into();
        setup.review_rule();
        assert_eq!(setup.pending, Some(add("deny", "Read(./secrets/**)", "localSettings")));
        setup.cancel();
        assert_eq!(setup.pending, None);
        assert!(setup.form.is_some(), "cancel goes back to the form");
    }

    #[test]
    fn a_change_runs_once_from_its_confirm_and_never_before() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kept = calls.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            Ok(if method == "spawn" { json!({ "code": 0, "stderr": "" }) } else { Value::Null })
        });
        let mut setup = Setup::default();
        setup.open("/permissions");
        setup.new_rule();
        setup.form.as_mut().unwrap().rule = "Bash(npm test)".into();
        setup.review_rule();
        assert!(calls.borrow().is_empty(), "asking runs nothing");
        let edit = setup.confirm().expect("the change shown");
        assert_eq!(setup.confirm(), None, "a second click while it is made");
        setup.ask(add("deny", "Bash(ls)", "userSettings"));
        assert_eq!(setup.pending, Some(edit.clone()), "nothing else is asked while it is made");
        let made = edit.clone();
        let outcome = task::block_on(async move { run(Some("5b7e2c1a-0f3d-4c8e-9a61-2d4f8b3c7e90"), "/work/shop", &made).await });
        assert_eq!(outcome, Ok(Vec::new()));
        let calls = calls.borrow();
        let methods: Vec<&str> = calls.iter().map(|(method, _)| method.as_str()).collect();
        // One run, whose input closes once the edit is written.
        assert_eq!(methods, ["spawn", "processStop"]);
        let spawn = &calls[0].1;
        assert_eq!((&spawn["program"], &spawn["args"], &spawn["cwd"]), (&json!("claude"), &json!(["edit-permission-rules", "--json"]), &json!("/work/shop")));
        // Under the account the chat's session is in.
        assert_eq!(spawn["account"], json!({ "session": "5b7e2c1a-0f3d-4c8e-9a61-2d4f8b3c7e90" }));
        let input: Value = serde_json::from_str(spawn["input"].as_str().unwrap()).unwrap();
        assert_eq!(input, json!({ "op": "add", "rules": ["Bash(npm test)"], "behavior": "allow", "destination": "localSettings" }));
        assert_eq!(calls[1].1["token"], spawn["token"]);
        setup.saved(&edit, outcome);
        assert_eq!(setup.confirm(), None, "a click after it was made");
    }

    #[test]
    fn a_refused_change_says_why_and_keeps_the_rule_you_wrote() {
        let refused = Ok(json!({ "code": 1, "stderr": "the rule looks like it contains a secret, so it was not saved to settings\n" }));
        let why = "the rule looks like it contains a secret, so it was not saved to settings";
        assert_eq!(outcome(&refused, &Value::Null), Err(why.into()));
        // edit-hook prints its refusal as JSON too, and says it on stderr.
        let gone = "edit_hook: that hook is no longer in localSettings; it may have been changed outside this dialog";
        let printed = json!({ "ok": false, "error": gone, "code": "target_missing" });
        assert_eq!(outcome(&Ok(json!({ "code": 1, "stderr": format!("{gone}\n") })), &printed), Err(gone.into()));
        assert_eq!(outcome(&Ok(json!({ "code": 1, "stderr": "" })), &Value::Null), Err("claude exited with code 1.".into()));
        assert_eq!(outcome(&Err("A plugin can keep at most 8 programs running".into()), &Value::Null), Err("A plugin can keep at most 8 programs running".into()));

        let mut setup = Setup::default();
        setup.open("/permissions");
        setup.new_rule();
        setup.form.as_mut().unwrap().rule = "Bash(curl -u admin:hunter2 https://ci.example.com)".into();
        setup.review_rule();
        let edit = setup.confirm().unwrap();
        setup.saved(&edit, outcome(&refused, &Value::Null));
        assert_eq!(setup.outcome, [("danger", why.to_owned())]);
        assert_eq!((&setup.pending, setup.saving), (&None, false));
        assert_eq!(setup.form.map(|form| form.rule), Some("Bash(curl -u admin:hunter2 https://ci.example.com)".into()));
    }

    #[test]
    fn a_saved_change_says_what_it_did_and_reads_its_list_again() {
        let warning = "\"Bash(*)\" was saved as the tool-wide rule \"Bash\", which matches every use of the tool. To limit it, put a specific pattern inside the parentheses.";
        let printed = json!({ "ok": true, "warnings": [warning], "stored": ["Bash"] });
        let warnings = outcome(&Ok(json!({ "code": 0, "stderr": "" })), &printed);
        let mut session = heard();
        let setup = &mut session.setup;
        setup.behavior = "ask".into();
        setup.new_rule();
        setup.form.as_mut().unwrap().rule = "Bash(*)".into();
        setup.review_rule();
        let edit = setup.confirm().unwrap();
        setup.saved(&edit, warnings);
        assert_eq!(setup.outcome, [("success", "Added Bash(*) to the ask rules in Local settings.".to_owned()), ("warn", warning.to_owned())]);
        assert_eq!(setup.form, None);
        let reread: Value = serde_json::from_str(edit.reread().trim()).unwrap();
        assert_eq!((&reread["request_id"], &reread["request"]["subtype"]), (&json!(RULES), &json!("list_permission_rules")));
        // Its answer is the list the dialog shows from then on.
        let listed = json!({ "subtype": "success", "request_id": RULES, "response": { "state": { "rules": [
            { "behavior": "ask", "source": "localSettings", "rule": "Bash", "editability": "persistent" }] } } });
        assert!(setup.answered(RULES, &listed));
        assert_eq!(setup.rules.clone().unwrap().unwrap().rules.iter().map(|rule| rule.rule.as_str()).collect::<Vec<_>>(), ["Bash"]);
        let hooks = setup.hooks.clone().unwrap().unwrap();
        let reads = |edit: Edit| serde_json::from_str::<Value>(edit.reread().trim()).unwrap()["request"]["subtype"].clone();
        assert_eq!(reads(Edit::remove_hook(&hooks.hooks[0]).unwrap()), "get_hooks_listing");
        assert_eq!(reads(Edit::Memory { setting: "autoMemoryEnabled", on: false }), "get_memory_dialog");
    }
}
